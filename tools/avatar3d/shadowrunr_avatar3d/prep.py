# SPDX-License-Identifier: Apache-2.0
"""The skull alone, square: what the image-to-3D models see.

The 2D rig already separates the skull from everything the 3D face must not mesh: its ``head``
and ``jaw`` layers hold the bone with the crystal in front of the brow inpainted away, the eye
fire keyed out and the monocle ring lifted. Composed over black they are a clean skull, and their
alpha is the matte. Two inputs come out of that matte:

- ``clean``: the rig's head and jaw layers, the bone as it is under the lights.
- ``portrait``: the portrait's own pixels inside the same matte (eye fire and ring included).

Both are padded square and written as RGBA with the matte as alpha. ``prep.json`` records the
square's place in the portrait, so later steps can map an input pixel back to the portrait.
"""

from __future__ import annotations

import hashlib
import json
from pathlib import Path

import numpy as np

OUT = 1024
MARGIN = 0.08  # of the square's side, on every edge


def sha256(path: Path) -> str:
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def skull_layers(rig: Path, size: tuple[int, int]) -> np.ndarray:
    """The rig's head and jaw layers composed over black: (h, w, 4) uint8, alpha = coverage."""
    from PIL import Image

    spec = json.loads((rig / "layers.json").read_text(encoding="utf-8"))
    canvas = Image.new("RGBA", size, (0, 0, 0, 0))
    for layer in sorted(spec["layers"], key=lambda l: l["z"]):
        if layer["name"] in ("head", "jaw"):
            x, y = layer["bbox"][:2]
            canvas.alpha_composite(Image.open(rig / layer["file"]).convert("RGBA"), (x, y))
    return np.asarray(canvas)


def square_box(alpha: np.ndarray, margin: float = MARGIN) -> tuple[int, int, int]:
    """(x0, y0, side) of the padded square around the matte, in source pixels."""
    ys, xs = np.nonzero(alpha > 8)
    x0, x1, y0, y1 = xs.min(), xs.max() + 1, ys.min(), ys.max() + 1
    side = int(round(max(x1 - x0, y1 - y0) / (1 - 2 * margin)))
    cx, cy = (x0 + x1) / 2, (y0 + y1) / 2
    return int(round(cx - side / 2)), int(round(cy - side / 2)), side


def crop_square(img: np.ndarray, box: tuple[int, int, int], out: int = OUT) -> np.ndarray:
    """Crop a square that may run off the picture (padded with zeros), resized to ``out``."""
    from PIL import Image

    x0, y0, side = box
    h, w = img.shape[:2]
    sq = np.zeros((side, side) + img.shape[2:], img.dtype)
    sx0, sy0 = max(x0, 0), max(y0, 0)
    sx1, sy1 = min(x0 + side, w), min(y0 + side, h)
    sq[sy0 - y0 : sy1 - y0, sx0 - x0 : sx1 - x0] = img[sy0:sy1, sx0:sx1]
    return np.asarray(Image.fromarray(sq).resize((out, out), Image.LANCZOS))


BONE_RGB = np.array([0.792, 0.722, 0.620])  # the portrait's bone, median of its sunlit cheek


def _smooth(x: np.ndarray, t: float, e0: float, e1: float) -> np.ndarray:
    u = np.clip((t - e0) / (e1 - e0), 0.0, 1.0)
    return u * u * (3 - 2 * u) * x


def _box(img: np.ndarray, r: int) -> np.ndarray:
    """Mean over a (2r+1) square, edge-clamped; separable, numpy only."""
    out = img.astype(np.float64)
    for axis in (0, 1):
        pad = [(0, 0)] * out.ndim
        pad[axis] = (r + 1, r)
        c = np.cumsum(np.pad(out, pad, mode="edge"), axis=axis)
        n = out.shape[axis]
        hi = np.take(c, np.arange(2 * r + 1, 2 * r + 1 + n), axis=axis)
        lo = np.take(c, np.arange(0, n), axis=axis)
        out = (hi - lo) / (2 * r + 1)
    return out


def hue_deg(rgb: np.ndarray) -> np.ndarray:
    mx, mn = rgb.max(axis=-1), rgb.min(axis=-1)
    d = np.maximum(mx - mn, 1e-6)
    r, g, b = rgb[..., 0], rgb[..., 1], rgb[..., 2]
    h = np.where(mx == r, ((g - b) / d) % 6, np.where(mx == g, (b - r) / d + 2, (r - g) / d + 4))
    return h * 60.0


def desmear(rgba: np.ndarray) -> tuple[np.ndarray, np.ndarray]:
    """Take the orange out of the bone. The rig's ``head`` layer keeps a smear of the monocle
    ring's fire around the image-right socket (orange, saturated, hue 15-55 degrees) while real
    bone is a pale, low-saturation tan. Pixels whose colour leans far from the bone's tint are
    recoloured to it, keeping their luminance (so the strokes and cracks stay) with the brightest
    orange pulled toward the local bone's brightness. Teal sockets and black are untouched.

    ``rgba``: (h, w, 4) uint8. Returns (the fixed image, the mask in 0..1, float)."""
    f = rgba.astype(np.float64) / 255.0
    rgb, a = f[..., :3], f[..., 3]
    mx, mn = rgb.max(axis=-1), rgb.min(axis=-1)
    sat = (mx - mn) / np.maximum(mx, 1e-3)
    lum = rgb @ np.array([0.299, 0.587, 0.114])
    tint = BONE_RGB / (BONE_RGB @ np.array([0.299, 0.587, 0.114]))
    dev = np.linalg.norm(rgb / np.maximum(lum, 1e-3)[..., None] - tint, axis=-1)
    h = hue_deg(rgb)
    w = _smooth(np.ones_like(dev), dev, 0.10, 0.22)
    w = _smooth(w, sat, 0.17, 0.27) * (a > 0.3)
    w = _smooth(w, 62 - h, 0, 8) * _smooth(np.ones_like(w), h + 5, 0, 8)
    w = _smooth(w, mx, 0.05, 0.12)
    # Grow by 2 px, then soften so the edge of the repair is not a staircase.
    g = w.copy()
    for _ in range(2):
        for axis in (0, 1):
            g = np.maximum(g, np.maximum(np.roll(g, 1, axis), np.roll(g, -1, axis)))
    w = np.clip(_box(_box(g, 2), 2) * 1.6, 0.0, 1.0)
    # Bone brightness from the lit, untouched pixels around (wide), the repair's own light
    # (narrow), and its strokes (the ratio to that narrow mean) kept as the detail.
    lit = (a > 0.3) & (lum > 0.45) & (w < 0.05)
    big = _box(lum * lit, 14) / np.maximum(_box(lit.astype(float), 14), 1e-3)
    big = np.where(_box(lit.astype(float), 14) > 0.02, big, 0.7)
    local = _box(lum, 5)
    detail = np.clip(lum / np.maximum(local, 0.04), 0.75, 1.3)
    target = np.clip((0.6 * local + 0.4 * np.maximum(big * 0.92, local)) * detail, 0.0, 1.0)
    lift = _smooth(np.ones_like(lum), lum, 0.12, 0.4)  # shadow stays shadow: only the lit smear is brightened
    new_l = lum + lift * (target - lum)
    new = np.clip(new_l[..., None] * tint, 0.0, 1.0)
    out = rgb * (1 - w[..., None]) + new * w[..., None]
    res = np.dstack([out, a])
    return (res * 255).round().astype(np.uint8), w


def bust_layers(rig: Path, size: tuple[int, int]) -> np.ndarray:
    """The skull (``head`` and ``jaw``, smear taken out) over the rig's ``torso`` layer (collar,
    cloak, shoulders: the layer is already clean of the head), composed on transparent."""
    from PIL import Image

    spec = json.loads((rig / "layers.json").read_text(encoding="utf-8"))
    canvas = Image.new("RGBA", size, (0, 0, 0, 0))
    for layer in spec["layers"]:
        if layer["name"] == "torso":
            x, y = layer["bbox"][:2]
            canvas.alpha_composite(Image.open(rig / layer["file"]).convert("RGBA"), (x, y))
    skull, _ = desmear(skull_layers(rig, size))
    canvas.alpha_composite(Image.fromarray(skull, "RGBA"))
    return np.asarray(canvas)


def prep_bust(rig: Path, portrait: Path, work: Path) -> dict:
    """``input_bust.png``: skull, collar and shoulders in one picture, for the model that
    generates the collar. The skull has the ring's smear taken out like ``input_clean2``; the collar is untouched."""
    from PIL import Image

    work.mkdir(parents=True, exist_ok=True)
    pimg = np.asarray(Image.open(portrait).convert("RGB"))
    layers = bust_layers(rig, (pimg.shape[1], pimg.shape[0]))
    alpha = layers[..., 3]
    ys, xs = np.nonzero(alpha > 8)
    x0, x1, y0, y1 = xs.min(), xs.max() + 1, ys.min(), ys.max() + 1
    side = int(round(max(x1 - x0, y1 - y0) / (1 - 2 * 0.04)))
    box = (int(round((x0 + x1) / 2 - side / 2)), int(round((y0 + y1) / 2 - side / 2)), side)
    img = crop_square(layers, box)
    img = img.copy()
    img[..., :3] = (img[..., :3].astype(np.float32) * (img[..., 3:] / 255.0)).astype(np.uint8)
    Image.fromarray(img, "RGBA").save(work / "input_bust.png")
    meta = {"box": {"x0": box[0], "y0": box[1], "side": box[2]}, "out": OUT,
            "portrait_sha256": sha256(portrait)}
    (work / "bust.json").write_text(json.dumps(meta, indent=1), encoding="utf-8")
    print(f"prep: bust square {box} -> {work / 'input_bust.png'}")
    return meta


def prep(rig: Path, portrait: Path, work: Path) -> dict:
    from PIL import Image

    work.mkdir(parents=True, exist_ok=True)
    pimg = np.asarray(Image.open(portrait).convert("RGB"))
    layers = skull_layers(rig, (pimg.shape[1], pimg.shape[0]))
    alpha = layers[..., 3]
    box = square_box(alpha)
    clean = crop_square(layers, box)
    port = crop_square(np.dstack([pimg, alpha]), box)
    # Keep colour only under the matte, so no crystal or collar pixel leaks through a soft edge.
    for name, img in (("clean", clean), ("portrait", port)):
        img = img.copy()
        img[..., :3] = (img[..., :3].astype(np.float32) * (img[..., 3:] / 255.0)).astype(np.uint8)
        Image.fromarray(img, "RGBA").save(work / f"input_{name}.png")
        if name == "clean":
            # The same skull with the ring's orange smear taken out of the bone: what both models
            # should eat (``--input clean2``). The rig's own layers stay as they are.
            fixed, _ = desmear(img)
            Image.fromarray(fixed, "RGBA").save(work / "input_clean2.png")
    meta = {
        "version": 1,
        "portrait_sha256": sha256(portrait),
        "portrait_size": [int(pimg.shape[1]), int(pimg.shape[0])],
        "box": {"x0": box[0], "y0": box[1], "side": box[2]},
        "out": OUT,
    }
    (work / "prep.json").write_text(json.dumps(meta, indent=1), encoding="utf-8")
    print(f"prep: square {box} of the portrait -> {work / 'input_clean.png'}, input_portrait.png")
    prep_bust(rig, portrait, work)
    return meta
