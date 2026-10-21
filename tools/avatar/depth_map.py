# SPDX-License-Identifier: Apache-2.0
"""Malakor's depth map, made offline from the face rig (streamer-screen prototype B).

    python tools/avatar/depth_map.py <rig dir> [--method analytic-v1]

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
a ring, a forward-leaning torso slab). A learned monocular map would be another method that
returns the same `Surface`; everything after it (fill, encoding, the JSON) stays as it is.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import sys
import time
from dataclasses import dataclass, field
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


@dataclass
class Surface:
    """A method's result: the visible surface's depth in source px (toward the viewer is
    positive), where it is known (`known` 0..1), and the plane depth of each floating part."""

    depth: np.ndarray
    known: np.ndarray
    planes: dict[str, float]


def load_rig(folder: Path) -> Rig:
    raw = (folder / "layers.json").read_bytes()
    spec = json.loads(raw)
    if spec.get("rig_type") != "glow":
        raise ValueError(f"layers.json is a {spec.get('rig_type')} rig, not a glow rig")
    w, h = int(spec["width"]), int(spec["height"])
    rig = Rig(spec=spec, width=w, height=h)
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


def analytic_v1(rig: Rig) -> Surface:
    layers = rig.spec["layers"]
    kinds = {l["name"]: l.get("kind") for l in layers}
    zeros = np.zeros((rig.height, rig.width), np.float32)

    def cover(*wanted: str) -> np.ndarray:
        a = zeros.copy()
        for name, k in kinds.items():
            if k in wanted and name in rig.alpha:
                a = np.maximum(a, rig.alpha[name])
        return a

    body = cover("body")
    skull = cover("head")
    jaw = cover("aperture_lower", "aperture_upper")
    head_all = np.maximum(skull, jaw)

    # the head: the ellipsoid, carved where the skull is dark inside its own matte (the sockets,
    # the nasal cavity, the gaps between the teeth), the carving softened so it has no cliffs
    lum = zeros.copy()
    for name, k in kinds.items():
        if k in ("head", "aperture_lower", "aperture_upper") and name in rig.luma:
            a = rig.alpha[name]
            lum = lum * (1.0 - a) + rig.luma[name] * a
    # the head's region: its matte with the small gaps in it closed (between the cheekbone and the
    # jaw the collar shows through; a hole there would tear the head's mesh on every turn)
    closed = np.maximum(smoothstep(0.35, 0.65, head_all), smoothstep(0.3, 0.55, blur(head_all, CLOSE_PX)))
    inner = np.clip(blur(head_all, 6.0) * 2.0 - 1.0, 0.0, 1.0)  # away from the silhouette
    dark = smoothstep(0.32, 0.05, lum) * inner
    carve = blur(dark, 8.0)
    # the ellipsoid's rim is eased, so its edge is steep but not a cliff
    head_z = blur(head_surface(rig), 6.0) - CARVE_PX * carve
    # the jaw a little forward, its edge eased in
    head_z = head_z + JAW_FORWARD_PX * blur(jaw, 3.0)

    torso_z = torso_surface(rig)

    # the visible surface, back to front: torso, head, then the floating parts at their planes
    planes = crown_planes(rig)
    soft_body = smoothstep(0.35, 0.65, body)
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


METHODS: dict[str, Callable[[Rig], Surface]] = {"analytic-v1": analytic_v1}


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
        "inputs": {name: hashlib.sha256(data).hexdigest() for name, data in sorted(rig.files.items())},
    }
    (folder / "depth.json").write_text(json.dumps(meta, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    return meta


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description="Write depth.png and depth.json beside a face rig's layers.json.")
    ap.add_argument("rig", type=Path, help="the rig folder (holds layers.json)")
    ap.add_argument("--method", default="analytic-v1", choices=sorted(METHODS))
    args = ap.parse_args(argv)
    started = time.perf_counter()
    try:
        meta = make(args.rig, args.method)
    except (OSError, ValueError, KeyError) as error:
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
