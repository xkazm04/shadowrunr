# SPDX-License-Identifier: Apache-2.0
"""Malakor's depth map, made offline from the face rig (streamer-screen prototype B).

    python tools/avatar/depth_map.py <rig dir> [--method analytic-v1 | learned-v1] [--model-python <py>]

Reads the rig's `layers.json` and its PNGs and writes, into the same folder:

- `depth.png`: 16-bit grayscale at the rig's size, the visible surface's depth. Near is bright.
- `depth.json`: how to read it back (`near`/`far` in source px toward the viewer, `scale` px per
  step), the method, a plane depth per floating part, and the sha256 of every input.

The depth renderer (`apps/stage/src/avatar/depth/`) displaces its meshes by this map. With no
`depth.png` next to `layers.json`, the stage draws the glow face instead.

**Reproducible.** Same inputs, same bytes: no clock, no randomness, sorted keys. The rig is cut
from the owner's portrait, so the map is the owner's file too: it lives beside the rig under
`data/`, and is never committed.

**Methods.** A method turns the rig into a `Surface`: the depth of each visible pixel and the
plane depth of each floating part. `analytic-v1` builds it from the rig's geometry and its
layers (an ellipsoid head carved by the dark of its sockets, a forward jaw, the crown shards on
a ring, a forward-leaning torso slab). `learned-v1` puts a monocular depth model's map
(Depth-Anything-V2-Small) on the same geometry: the head calibrated to that ellipsoid, the torso
the same slab carved with the model's relief, the shards on the same planes. Everything after
the method (fill, encoding, the JSON) is shared.

**learned-v1 runs in two Pythons.** This script needs numpy and Pillow; the model step
(`learned_depth.py`) needs numpy and onnxruntime. `--method learned-v1` writes the model's input
beside the rig (`learned.in.npy`) and stops with exit 2 until `learned_depth.py` has written
`learned.out.npy` and `learned.json` for that very input; run again, it writes the map. With
`--model-python <python with onnxruntime>` (or `SHADOWRUNR_DEPTH_PYTHON`) it runs the model step
itself. The `learned.*` files are cut from the portrait too, and stay beside the rig.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import subprocess
import sys
import time
from dataclasses import dataclass, field
from io import BytesIO
from pathlib import Path
from typing import Callable

import numpy as np
from PIL import Image

VERSION = 1
LEVELS = 65535


@dataclass
class Rig:
    """The parts of a rig the depth methods read: its spec, and each layer's coverage and light
    as full-canvas float arrays (0..1)."""

    spec: dict
    width: int
    height: int
    alpha: dict[str, np.ndarray] = field(default_factory=dict)
    luma: dict[str, np.ndarray] = field(default_factory=dict)
    files: dict[str, bytes] = field(default_factory=dict)
    folder: Path | None = None


@dataclass
class Surface:
    """A method's result: the visible surface's depth in source px (toward the viewer is
    positive), where it is known (`known` 0..1), and the plane depth of each floating part."""

    depth: np.ndarray
    known: np.ndarray
    planes: dict[str, float]
    # more files the method read (named in `inputs`), and more keys for `depth.json`
    inputs: dict[str, bytes] = field(default_factory=dict)
    meta: dict = field(default_factory=dict)


def load_rig(folder: Path) -> Rig:
    raw = (folder / "layers.json").read_bytes()
    spec = json.loads(raw)
    if spec.get("rig_type") != "glow":
        raise ValueError(f"layers.json is a {spec.get('rig_type')} rig, not a glow rig")
    w, h = int(spec["width"]), int(spec["height"])
    rig = Rig(spec=spec, width=w, height=h, folder=folder)
    rig.files["layers.json"] = raw
    for layer in spec["layers"]:
        name, file = layer["name"], layer.get("file")
        if not file or layer.get("kind") == "static":
            continue  # the backplate is the page, not the figure
        data = (folder / file).read_bytes()
        rig.files[file] = data
        x, y, bw, bh = (int(round(v)) for v in layer.get("bbox", [0, 0, w, h]))
        with Image.open(folder / file) as im:
            px = np.asarray(im.convert("RGBA"), dtype=np.float32) / 255.0
        alpha = np.zeros((h, w), np.float32)
        luma = np.zeros((h, w), np.float32)
        ph, pw = px.shape[:2]
        # the layer's picture, placed at its bbox and clipped to the canvas
        x0, y0, x1, y1 = max(0, x), max(0, y), min(w, x + pw), min(h, y + ph)
        if x1 > x0 and y1 > y0:
            sub = px[y0 - y : y1 - y, x0 - x : x1 - x]
            alpha[y0:y1, x0:x1] = sub[..., 3]
            luma[y0:y1, x0:x1] = sub[..., 0] * 0.2126 + sub[..., 1] * 0.7152 + sub[..., 2] * 0.0722
        rig.alpha[name] = alpha
        rig.luma[name] = luma
    return rig


# ---------------------------------------------------------------- image helpers (numpy only)


def box_blur(a: np.ndarray, r: int) -> np.ndarray:
    """A separable box blur of radius `r` px, edges clamped."""
    if r <= 0:
        return a.copy()
    out = a.astype(np.float64)
    for axis in (0, 1):
        pad = [(0, 0), (0, 0)]
        pad[axis] = (r + 1, r)
        c = np.cumsum(np.pad(out, pad, mode="edge"), axis=axis)
        n = out.shape[axis]
        hi = np.take(c, np.arange(2 * r + 1, 2 * r + 1 + n), axis=axis)
        lo = np.take(c, np.arange(0, n), axis=axis)
        out = (hi - lo) / (2 * r + 1)
    return out.astype(np.float32)


def blur(a: np.ndarray, sigma: float) -> np.ndarray:
    """About a Gaussian of `sigma` px: three box blurs."""
    r = max(0, int(round(sigma * 0.9)))
    for _ in range(3):
        a = box_blur(a, r)
    return a


def push_fill(value: np.ndarray, known: np.ndarray, levels: int = 10) -> np.ndarray:
    """Fill where `known` is low by pushing the known values outward: a multi-scale normalised
    blur, finest first, so every pixel takes the nearest surface's depth and the figure's edge
    has no cliff to the empty canvas."""
    num0 = value * known
    est = np.zeros_like(value)
    have = np.zeros(value.shape, bool)
    for i in range(levels):
        r = 2 ** (i + 1)
        den = box_blur(known, r)
        take = (den > 1e-4) & ~have
        if take.any():
            est[take] = (box_blur(num0, r)[take] / den[take]).astype(np.float32)
            have |= take
        if have.all():
            break
    return (value * known + (1.0 - known) * est).astype(np.float32)


def smoothstep(e0: float, e1: float, x: np.ndarray) -> np.ndarray:
    t = np.clip((x - e0) / (e1 - e0), 0.0, 1.0)
    return t * t * (3.0 - 2.0 * t)


# ---------------------------------------------------------------- the analytic method

# Depths in source px, toward the viewer. The head's ellipsoid is this deep for its width, sits
# this far in front of the torso, and its sockets are carved this deep where the skull is dark.
HEAD_DEPTH_FRAC = 0.8
HEAD_FLOOR = 24.0
CARVE_PX = 36.0
# gaps in the head's matte up to about this wide are closed, so the head's surface has no holes
CLOSE_PX = 14.0
JAW_FORWARD_PX = 12.0
CROWN_DEPTH_FRAC = 0.75
TORSO_TOP_Z = -26.0
TORSO_LEAN_PX = 60.0
TORSO_BULGE_PX = 36.0


def head_surface(rig: Rig) -> np.ndarray:
    """The head's ellipsoid from `rig.head`, everywhere on the canvas (outside it, the floor)."""
    head = rig.spec["rig"]["head"]
    (cx, cy), (rx, ry) = head["center"], head["radius"]
    ys, xs = np.mgrid[0 : rig.height, 0 : rig.width].astype(np.float32)
    u = ((xs - cx) / rx) ** 2 + ((ys - cy) / ry) ** 2
    return HEAD_FLOOR + HEAD_DEPTH_FRAC * rx * np.sqrt(np.clip(1.0 - u, 0.0, 1.0))


def torso_surface(rig: Rig) -> np.ndarray:
    """A slab leaning toward the viewer (its top nearer than its bottom), bulging a little across."""
    head = rig.spec["rig"]["head"]
    cx = head["center"][0]
    top = float(head.get("pivot_y", head["chin_y"])) - 0.7 * head["radius"][1]
    ys, xs = np.mgrid[0 : rig.height, 0 : rig.width].astype(np.float32)
    down = np.clip((ys - top) / max(1.0, rig.height - top), 0.0, 1.0)
    across = np.clip(1.0 - ((xs - cx) / (0.6 * rig.width)) ** 2, 0.0, 1.0)
    return TORSO_TOP_Z - TORSO_LEAN_PX * down + TORSO_BULGE_PX * np.sqrt(across)


def crown_planes(rig: Rig) -> dict[str, float]:
    """Each floating part's plane: the shards stand on a ring round the skull, nearer the further
    forward their place on it, and as far forward as they follow the head."""
    head = rig.spec["rig"]["head"]
    cx, rx = head["center"][0], head["radius"][0]
    floats = [l for l in rig.spec["layers"] if l.get("kind") == "float" and l["name"] in rig.alpha]
    if not floats:
        return {}

    def centre_x(l: dict) -> float:
        a = l.get("anchor")
        if a:
            return float(a[0])
        x, _, w, _ = l["bbox"]
        return x + w / 2.0

    ring = max(abs(centre_x(l) - cx) for l in floats) * 1.08 + 1.0
    planes = {}
    for l in floats:
        dx = centre_x(l) - cx
        on_ring = np.sqrt(max(0.0, 1.0 - (dx / ring) ** 2))
        follow = float(l.get("follow", 1.0))
        planes[l["name"]] = round(HEAD_FLOOR + CROWN_DEPTH_FRAC * HEAD_DEPTH_FRAC * rx * on_ring * follow, 3)
    return planes


def cover(rig: Rig, *wanted: str) -> np.ndarray:
    """The union of the coverage of the layers of these kinds."""
    a = np.zeros((rig.height, rig.width), np.float32)
    for layer in rig.spec["layers"]:
        if layer.get("kind") in wanted and layer["name"] in rig.alpha:
            a = np.maximum(a, rig.alpha[layer["name"]])
    return a


def head_region(head_all: np.ndarray) -> np.ndarray:
    """The head's region: its matte with the small gaps in it closed (between the cheekbone and the
    jaw the collar shows through; a hole there would tear the head's mesh on every turn)."""
    return np.maximum(smoothstep(0.35, 0.65, head_all), smoothstep(0.3, 0.55, blur(head_all, CLOSE_PX)))


def analytic_v1(rig: Rig) -> Surface:
    layers = rig.spec["layers"]
    kinds = {l["name"]: l.get("kind") for l in layers}
    zeros = np.zeros((rig.height, rig.width), np.float32)

    skull = cover(rig, "head")
    jaw = cover(rig, "aperture_lower", "aperture_upper")
    head_all = np.maximum(skull, jaw)

    # the head: the ellipsoid, carved where the skull is dark inside its own matte (the sockets,
    # the nasal cavity, the gaps between the teeth), the carving softened so it has no cliffs
    lum = zeros.copy()
    for name, k in kinds.items():
        if k in ("head", "aperture_lower", "aperture_upper") and name in rig.luma:
            a = rig.alpha[name]
            lum = lum * (1.0 - a) + rig.luma[name] * a
    closed = head_region(head_all)
    inner = np.clip(blur(head_all, 6.0) * 2.0 - 1.0, 0.0, 1.0)  # away from the silhouette
    dark = smoothstep(0.32, 0.05, lum) * inner
    carve = blur(dark, 8.0)
    # the ellipsoid's rim is eased, so its edge is steep but not a cliff
    head_z = blur(head_surface(rig), 6.0) - CARVE_PX * carve
    # the jaw a little forward, its edge eased in
    head_z = head_z + JAW_FORWARD_PX * blur(jaw, 3.0)

    return layered(rig, torso_surface(rig), head_z, closed)


def layered(rig: Rig, torso_z: np.ndarray, head_z: np.ndarray, closed: np.ndarray) -> Surface:
    """The visible surface, back to front: the torso, the head over its closed region, then the
    floating parts at their planes (`crown_planes`, whatever the method)."""
    planes = crown_planes(rig)
    soft_body = smoothstep(0.35, 0.65, cover(rig, "body"))
    depth = torso_z.copy()
    known = soft_body
    hard = closed
    depth = depth * (1.0 - hard) + head_z * hard
    known = np.maximum(known, hard)
    for name, z in planes.items():
        a = smoothstep(0.35, 0.65, rig.alpha[name])
        depth = depth * (1.0 - a) + z * a
        known = np.maximum(known, a)
    return Surface(depth=depth.astype(np.float32), known=known.astype(np.float32), planes=planes)


# ---------------------------------------------------------------- the learned method

# The model's input: the portrait at this square size (a multiple of the model's 14 px patch; the
# Small model's own processor size), bicubic, normalised as the model was trained.
MODEL_SIDE = 518
MODEL_MEAN = (0.485, 0.456, 0.406)
MODEL_STD = (0.229, 0.224, 0.225)
# the files beside the rig the model step reads and writes (`learned_depth.py`)
LEARNED_IN = "learned.in.npy"
LEARNED_OUT = "learned.out.npy"
LEARNED_META = "learned.json"
# The calibration's two anchors on the head's ellipsoid, as fractions of its radii: the centre
# disc, and the ring inside the rim.
ANCHOR_CENTRE = 0.35
ANCHOR_RIM = (0.8, 0.95)
# The model's depth is trusted this far (px) inside a matte; nearer the edge it blends with the
# backplate it was shown, so the edge takes the depth just inside it.
EDGE_PX = 4.0
# How far past the analytic head and torso the calibrated depth may reach, as a fraction of the
# head's depth, before it is clipped (a stray value would move the renderer's pivot).
REACH = 0.35
# The torso's relief: the model's depth less its own blur at this sigma (px), at most this deep.
RELIEF_SIGMA = 24.0
RELIEF_MAX = 40.0


class LearnedPending(Exception):
    """The model step has not run on the current input (or ran on another one)."""


def portrait(rig: Rig) -> tuple[np.ndarray, dict[str, bytes]]:
    """The rig recomposed at rest, backplate included (what the model is shown): the cut layers
    over each other, the light layers added. Returns the RGB (0..1) and the files it read beyond
    `rig.files`."""
    assert rig.folder is not None
    w, h = rig.width, rig.height
    rgb = np.zeros((h, w, 3), np.float32)
    extra: dict[str, bytes] = {}
    for layer in rig.spec["layers"]:
        file = layer.get("file")
        if not file:
            continue
        data = rig.files.get(file)
        if data is None:
            data = (rig.folder / file).read_bytes()
            extra[file] = data
        with Image.open(BytesIO(data)) as im:
            px = np.asarray(im.convert("RGBA"), dtype=np.float32) / 255.0
        x, y, _, _ = (int(round(v)) for v in layer.get("bbox", [0, 0, w, h]))
        ph, pw = px.shape[:2]
        x0, y0, x1, y1 = max(0, x), max(0, y), min(w, x + pw), min(h, y + ph)
        if x1 <= x0 or y1 <= y0:
            continue
        sub = px[y0 - y : y1 - y, x0 - x : x1 - x]
        a = sub[..., 3:4]
        if layer.get("mode") == "light":
            rgb[y0:y1, x0:x1] += sub[..., :3] * a
        else:
            rgb[y0:y1, x0:x1] = rgb[y0:y1, x0:x1] * (1.0 - a) + sub[..., :3] * a
    return np.clip(rgb, 0.0, 1.0), extra


def model_input(rgb: np.ndarray) -> bytes:
    """The model's input as `.npy` bytes: 1x3xSxS float32, resized and normalised."""
    im = Image.fromarray(np.rint(rgb * 255.0).astype(np.uint8), "RGB")
    small = np.asarray(im.resize((MODEL_SIDE, MODEL_SIDE), Image.Resampling.BICUBIC), dtype=np.float32) / 255.0
    norm = (small - np.array(MODEL_MEAN, np.float32)) / np.array(MODEL_STD, np.float32)
    pixels = np.ascontiguousarray(norm.transpose(2, 0, 1)[None].astype(np.float32))
    buf = BytesIO()
    np.save(buf, pixels, allow_pickle=False)
    return buf.getvalue()


def write_learned_input(rig: Rig) -> tuple[bytes, dict[str, bytes]]:
    """Write `learned.in.npy` beside the rig unless it already holds the current input."""
    assert rig.folder is not None
    rgb, extra = portrait(rig)
    raw = model_input(rgb)
    path = rig.folder / LEARNED_IN
    if not path.is_file() or path.read_bytes() != raw:
        path.write_bytes(raw)
    return raw, extra


def read_learned(rig: Rig, raw_in: bytes) -> tuple[np.ndarray, dict]:
    """The model's output for `raw_in`, upsampled to the rig, and the model step's record."""
    assert rig.folder is not None
    meta_path, out_path = rig.folder / LEARNED_META, rig.folder / LEARNED_OUT
    if not meta_path.is_file() or not out_path.is_file():
        raise LearnedPending(f"no {LEARNED_OUT} beside the rig yet")
    meta = json.loads(meta_path.read_text(encoding="utf-8"))
    out_raw = out_path.read_bytes()
    if meta.get("input_sha256") != hashlib.sha256(raw_in).hexdigest():
        raise LearnedPending(f"{LEARNED_OUT} was made from another input")
    if meta.get("output_sha256") != hashlib.sha256(out_raw).hexdigest():
        raise LearnedPending(f"{LEARNED_OUT} is not the file {LEARNED_META} names")
    out = np.load(BytesIO(out_raw), allow_pickle=False).astype(np.float32)
    if out.ndim != 2 or not np.isfinite(out).all():
        raise ValueError(f"{LEARNED_OUT} is not a finite 2-D map")
    big = Image.fromarray(out, "F").resize((rig.width, rig.height), Image.Resampling.BICUBIC)
    return np.asarray(big, dtype=np.float32), meta


@dataclass
class Calibration:
    """The model's depth `d` in source px: `gain * d + offset`, less the tilt (px at the head's
    radius, across and down) the model leans the head by."""

    gain: float
    offset: float
    tilt: tuple[float, float]

    def head(self, rig: Rig, d: np.ndarray) -> np.ndarray:
        head = rig.spec["rig"]["head"]
        (cx, cy), (rx, ry) = head["center"], head["radius"]
        ys, xs = np.mgrid[0 : rig.height, 0 : rig.width].astype(np.float32)
        plane = self.tilt[0] * (xs - cx) / rx + self.tilt[1] * (ys - cy) / ry
        return (self.gain * d + self.offset - plane).astype(np.float32)


def calibrate(rig: Rig, d: np.ndarray, head_all: np.ndarray) -> Calibration:
    """Put the model's depth `d` (nearer is larger, any scale and offset) onto the head's
    ellipsoid: its median over the centre disc to the ellipsoid's there, and over the ring inside
    the rim to the ellipsoid's there, both inside the head's matte. Then the plane the result
    leans by against the ellipsoid (the model reads the bottom of a portrait as nearer) is
    measured, to be taken off: the rig's head faces the viewer."""
    head = rig.spec["rig"]["head"]
    (cx, cy), (rx, ry) = head["center"], head["radius"]
    ys, xs = np.mgrid[0 : rig.height, 0 : rig.width].astype(np.float32)
    u, v = (xs - cx) / rx, (ys - cy) / ry
    r = np.sqrt(u**2 + v**2)
    inside = head_all > 0.5
    ellipsoid = head_surface(rig)
    centre = inside & (r <= ANCHOR_CENTRE)
    rim = inside & (r >= ANCHOR_RIM[0]) & (r <= ANCHOR_RIM[1])
    if centre.sum() < 16 or rim.sum() < 16:
        raise ValueError("the head's matte does not cover its centre and rim; cannot calibrate the model")
    dc, dr = float(np.median(d[centre])), float(np.median(d[rim]))
    zc, zr = float(np.median(ellipsoid[centre])), float(np.median(ellipsoid[rim]))
    if dc - dr <= 1e-6 * max(1.0, abs(dc)):
        raise ValueError("the model does not put the head's centre before its rim; refusing to calibrate")
    gain = (zc - zr) / (dc - dr)
    offset = zc - gain * dc
    body = inside & (r <= ANCHOR_RIM[1])
    res = (gain * d[body] + offset - ellipsoid[body]).astype(np.float64)
    basis = np.stack([np.ones_like(res), u[body].astype(np.float64), v[body].astype(np.float64)], 1)
    coef = np.linalg.lstsq(basis, res, rcond=None)[0]
    return Calibration(gain=gain, offset=offset, tilt=(float(coef[1]), float(coef[2])))


def learned_v1(rig: Rig) -> Surface:
    """The model's relative depth inside the figure, on the rig's geometry:

    - the head: calibrated to the head's ellipsoid (so a turn keeps its size), its lean taken off;
    - the torso: the analytic slab, carved with the model's relief at the head's scale (the folds,
      the collar, the clasp), since across a region that large the model's depth follows the
      picture's framing more than the figure;
    - each matte's edge takes the depth just inside it, the head's region is closed as
      `analytic-v1` closes it, and the floating parts stand on their planes."""
    raw_in, extra = write_learned_input(rig)
    d, step = read_learned(rig, raw_in)
    skull = cover(rig, "head")
    jaw = cover(rig, "aperture_lower", "aperture_upper")
    head_all = np.maximum(skull, jaw)
    body = cover(rig, "body")

    def trusted(matte: np.ndarray) -> np.ndarray:
        return smoothstep(0.85, 1.0, blur(matte, EDGE_PX))

    cal = calibrate(rig, d, head_all)
    deep = HEAD_DEPTH_FRAC * rig.spec["rig"]["head"]["radius"][0]
    lo, hi = TORSO_TOP_Z - TORSO_LEAN_PX - REACH * deep, HEAD_FLOOR + deep * (1.0 + REACH)
    head_raw = push_fill(cal.head(rig, d), trusted(head_all))
    in_head = head_region(head_all) > 0.5
    clipped = float(np.mean((head_raw[in_head] < lo) | (head_raw[in_head] > hi))) if in_head.any() else 0.0
    head_z = np.clip(head_raw, lo, hi)

    d_body = push_fill(d, trusted(body))
    relief = np.clip(cal.gain * (d_body - blur(d_body, RELIEF_SIGMA)), -RELIEF_MAX, RELIEF_MAX)
    torso_z = torso_surface(rig) + relief

    surface = layered(rig, torso_z.astype(np.float32), head_z.astype(np.float32), head_region(head_all))
    surface.inputs = extra
    surface.meta = {
        "model": step["model"],
        "calibration": {
            "gain": round(cal.gain, 6),
            "offset": round(cal.offset, 3),
            "tilt": [round(cal.tilt[0], 3), round(cal.tilt[1], 3)],
            "clipped": round(clipped, 6),
        },
    }
    return surface


METHODS: dict[str, Callable[[Rig], Surface]] = {"analytic-v1": analytic_v1, "learned-v1": learned_v1}


# ---------------------------------------------------------------- output


def encode(depth: np.ndarray) -> tuple[np.ndarray, float, float]:
    """16-bit levels, near bright: (levels, near, far) with near/far rounded to 1/1000 px."""
    near = round(float(depth.max()), 3)
    far = round(float(depth.min()), 3)
    span = max(near - far, 1e-3)
    levels = np.rint(np.clip((depth - far) / span, 0.0, 1.0) * LEVELS).astype(np.uint16)
    return levels, near, far


def decode(levels: np.ndarray, near: float, far: float) -> np.ndarray:
    return far + levels.astype(np.float32) / LEVELS * (near - far)


def make(folder: Path, method: str = "analytic-v1") -> dict:
    """Write `depth.png` and `depth.json` into the rig folder; returns the JSON written."""
    if method not in METHODS:
        raise ValueError(f"unknown method {method}; known: {', '.join(sorted(METHODS))}")
    rig = load_rig(folder)
    surface = METHODS[method](rig)
    depth = push_fill(surface.depth, surface.known)
    levels, near, far = encode(depth)
    # a uint16 array is Pillow's "I;16", which it writes as a 16-bit grayscale PNG
    Image.fromarray(levels).save(folder / "depth.png", format="PNG", optimize=False)
    meta = {
        "version": VERSION,
        "method": method,
        "width": rig.width,
        "height": rig.height,
        "units": "source_px",
        "near": near,
        "far": far,
        "scale": round((near - far) / LEVELS, 9),
        "planes": surface.planes,
        "inputs": {
            name: hashlib.sha256(data).hexdigest() for name, data in sorted({**rig.files, **surface.inputs}.items())
        },
        **surface.meta,
    }
    (folder / "depth.json").write_text(json.dumps(meta, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    return meta


def run_model_step(folder: Path, python: Path, model: Path | None = None) -> None:
    """Run `learned_depth.py` on the rig with `python` (one that has onnxruntime)."""
    cmd = [str(python), "-I", str(Path(__file__).with_name("learned_depth.py")), str(folder)]
    if model is not None:
        cmd += ["--model", str(model)]
    flags = getattr(subprocess, "CREATE_NO_WINDOW", 0)
    done = subprocess.run(cmd, creationflags=flags, check=False)
    if done.returncode != 0:
        raise ValueError(f"the model step failed (exit {done.returncode})")


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description="Write depth.png and depth.json beside a face rig's layers.json.")
    ap.add_argument("rig", type=Path, help="the rig folder (holds layers.json)")
    ap.add_argument("--method", default="analytic-v1", choices=sorted(METHODS))
    ap.add_argument(
        "--model-python",
        type=Path,
        default=os.environ.get("SHADOWRUNR_DEPTH_PYTHON") or None,
        help="learned-v1: a python with onnxruntime to run the model step with (else it is run by hand)",
    )
    ap.add_argument("--model", type=Path, default=None, help="learned-v1: the model file, passed to the model step")
    args = ap.parse_args(argv)
    started = time.perf_counter()
    try:
        try:
            meta = make(args.rig, args.method)
        except LearnedPending as pending:
            if not args.model_python:
                print(
                    f"depth_map: {pending}. Run the model step on {args.rig / LEARNED_IN}:\n"
                    f"  <python with onnxruntime> {Path(__file__).with_name('learned_depth.py')} {args.rig}\n"
                    f"then this again (or pass --model-python).",
                    file=sys.stderr,
                )
                return 2
            run_model_step(args.rig, Path(args.model_python), args.model)
            meta = make(args.rig, args.method)
    except (OSError, ValueError, KeyError, LearnedPending) as error:
        print(f"depth_map: {error}", file=sys.stderr)
        return 1
    took = time.perf_counter() - started
    print(
        f"depth_map: {args.rig / 'depth.png'} ({meta['width']}x{meta['height']}, {meta['method']}, "
        f"near {meta['near']} far {meta['far']} px) in {took:.2f} s"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
