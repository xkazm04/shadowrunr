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
    meta = {
        "version": 1,
        "portrait_sha256": sha256(portrait),
        "portrait_size": [int(pimg.shape[1]), int(pimg.shape[0])],
        "box": {"x0": box[0], "y0": box[1], "side": box[2]},
        "out": OUT,
    }
    (work / "prep.json").write_text(json.dumps(meta, indent=1), encoding="utf-8")
    print(f"prep: square {box} of the portrait -> {work / 'input_clean.png'}, input_portrait.png")
    return meta
