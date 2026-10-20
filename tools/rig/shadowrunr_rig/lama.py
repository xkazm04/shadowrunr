# SPDX-License-Identifier: Apache-2.0
"""LaMa inpainting (big-lama, Apache-2.0, a TorchScript file), loaded lazily from a path you give.

The model is the operator's file: nothing here downloads it, and the repo ships no weights. torch
is imported when the first fill runs, so the rest of the splitter (and its tests) never needs it.
"""

from __future__ import annotations

from pathlib import Path

import numpy as np


class Lama:
    """Fills masked pixels of an RGB picture from what surrounds them."""

    def __init__(self, model: Path, device: str = "cuda"):
        self.model_path = Path(model)
        if not self.model_path.is_file():
            raise FileNotFoundError(f"no LaMa model at {self.model_path}")
        self.device = device
        self._net = None

    def _load(self):
        if self._net is None:
            import torch

            if self.device.startswith("cuda") and not torch.cuda.is_available():
                self.device = "cpu"
            self._net = torch.jit.load(str(self.model_path), map_location=self.device).eval()
        return self._net

    def fill(self, rgb: np.ndarray, mask: np.ndarray) -> np.ndarray:
        """`rgb` (H, W, 3) 0..255 with the pixels under `mask` replaced by LaMa's fill."""
        import torch

        net = self._load()
        m = (mask > 0).astype(np.float32)
        src = np.clip(rgb, 0, 255).astype(np.float32)
        if m.sum() == 0:
            return src.copy()
        h, w = m.shape
        ph, pw = (8 - h % 8) % 8, (8 - w % 8) % 8
        img = np.pad(src / 255.0, ((0, ph), (0, pw), (0, 0)), mode="reflect")
        mm = np.pad(m, ((0, ph), (0, pw)), mode="reflect")
        with torch.no_grad():
            ti = torch.from_numpy(np.ascontiguousarray(img.transpose(2, 0, 1)))[None].to(
                self.device
            )
            tm = torch.from_numpy(np.ascontiguousarray(mm))[None, None].to(self.device)
            out = net(ti, tm)[0].permute(1, 2, 0).clamp(0, 1).cpu().numpy()
        out = out[:h, :w] * 255.0
        res = src.copy()
        res[m > 0] = out[m > 0]
        return res


def fill_crop(fill, rgb: np.ndarray, mask: np.ndarray, margin: float = 0.6) -> np.ndarray:
    """Run `fill` on the mask's bbox plus a context margin (better detail for small holes)."""
    ys, xs = np.nonzero(mask > 0)
    if len(xs) == 0:
        return np.asarray(rgb, np.float32).copy()
    h, w = mask.shape
    bw, bh = xs.max() - xs.min() + 1, ys.max() - ys.min() + 1
    mx, my = int(bw * margin) + 16, int(bh * margin) + 16
    x0, x1 = max(0, xs.min() - mx), min(w, xs.max() + 1 + mx)
    y0, y1 = max(0, ys.min() - my), min(h, ys.max() + 1 + my)
    out = np.asarray(rgb, np.float32).copy()
    sub = fill(out[y0:y1, x0:x1], mask[y0:y1, x0:x1])
    region = mask[y0:y1, x0:x1] > 0
    out[y0:y1, x0:x1][region] = sub[region]
    return out
