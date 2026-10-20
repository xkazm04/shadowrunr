# SPDX-License-Identifier: Apache-2.0
"""Mask and region maths for the splitter, numpy only: polygons, morphology, soft edges, bboxes.

Ported from athena's t1-rig prep (``layerkit.py`` and ``split_glow.py``), which used OpenCV; the
same operations are written here on numpy so the splitter's pure parts need nothing else. Masks
are ``bool`` arrays and coverages ``float32`` arrays in 0..1, both (H, W). Coordinates are source
pixels with the origin at the top-left corner; a pixel's centre is at (x + 0.5, y + 0.5).
"""

from __future__ import annotations

from collections.abc import Sequence

import numpy as np

Point = Sequence[float]


def poly_mask(shape: tuple[int, int], pts: Sequence[Point]) -> np.ndarray:
    """The pixels whose centres lie inside the polygon (even-odd rule)."""
    h, w = shape
    p = np.asarray(pts, np.float64)
    if p.ndim != 2 or p.shape[0] < 3 or p.shape[1] != 2:
        raise ValueError("a polygon needs at least three [x, y] points")
    out = np.zeros((h, w), bool)
    ys = np.arange(h) + 0.5
    xs = np.arange(w) + 0.5
    x0, y0 = p[:, 0], p[:, 1]
    x1, y1 = np.roll(x0, -1), np.roll(y0, -1)
    for ya, yb, xa, xb in zip(y0, y1, x0, x1, strict=True):
        if ya == yb:
            continue
        lo, hi = min(ya, yb), max(ya, yb)
        rows = np.nonzero((ys >= lo) & (ys < hi))[0]
        if len(rows) == 0:
            continue
        xc = xa + (ys[rows] - ya) * (xb - xa) / (yb - ya)
        # every crossing toggles the pixels to its right: parity counts the crossings
        cross = xs[None, :] > xc[:, None]
        out[rows] ^= cross
    return out


def mirror_points(pts: Sequence[Point], cx: float) -> list[list[float]]:
    """The polygon reflected about the vertical line x = cx."""
    return [[2 * cx - float(x), float(y)] for x, y in pts]


def _shift_or(m: np.ndarray, dy: int, dx: int) -> np.ndarray:
    h, w = m.shape
    out = np.zeros_like(m)
    ys, yd = (slice(0, h - dy), slice(dy, h)) if dy >= 0 else (slice(-dy, h), slice(0, h + dy))
    xs, xd = (slice(0, w - dx), slice(dx, w)) if dx >= 0 else (slice(-dx, w), slice(0, w + dx))
    out[yd, xd] = m[ys, xs]
    return out


_SQUARE = [(dy, dx) for dy in (-1, 0, 1) for dx in (-1, 0, 1) if dy or dx]
_CROSS = [(-1, 0), (1, 0), (0, -1), (0, 1)]


def dilate(mask: np.ndarray, r: int) -> np.ndarray:
    """Grow a mask by about `r` px: alternating square and cross steps, an octagon close to a disk."""
    m = mask.astype(bool)
    for i in range(max(0, int(r))):
        step = _SQUARE if i % 2 == 0 else _CROSS
        grown = m.copy()
        for dy, dx in step:
            grown |= _shift_or(m, dy, dx)
        m = grown
    return m


def erode(mask: np.ndarray, r: int) -> np.ndarray:
    """Shrink a mask by about `r` px (pixels past the border count as outside)."""
    m = mask.astype(bool)
    if r <= 0:
        return m.copy()
    padded = np.pad(~m, r, constant_values=True)
    return ~dilate(padded, r)[r:-r, r:-r]


def close(mask: np.ndarray, r: int) -> np.ndarray:
    """Close gaps narrower than about 2r px (dilate, then erode)."""
    if r <= 0:
        return mask.astype(bool)
    padded = np.pad(mask.astype(bool), r)
    return erode(dilate(padded, r), r)[r:-r, r:-r]


def _spread(reach: np.ndarray, free: np.ndarray) -> np.ndarray:
    """Every run of `free` along a row that touches `reach` is reached."""
    h, w = free.shape
    seg = np.cumsum(~free, axis=1) + np.arange(h)[:, None] * (w + 1)
    ids = seg[free]
    hit = np.zeros(int(seg.max()) + 1, bool)
    hit[ids[reach[free]]] = True
    out = np.zeros_like(free)
    out[free] = hit[ids]
    return out


def flood(seed: np.ndarray, free: np.ndarray) -> np.ndarray:
    """The part of `free` 4-connected to `seed` (a scanline flood, rows then columns, to a fixpoint)."""
    free = free.astype(bool)
    reach = seed.astype(bool) & free
    while True:
        nxt = _spread(reach, free)
        nxt = _spread(nxt.T, free.T).T
        if np.array_equal(nxt, reach):
            return reach
        reach = nxt


def fill_holes(mask: np.ndarray) -> np.ndarray:
    """The mask with every enclosed hole filled (what the border cannot reach is inside)."""
    m = mask.astype(bool)
    border = np.zeros_like(m)
    border[0, :] = border[-1, :] = True
    border[:, 0] = border[:, -1] = True
    outside = flood(border & ~m, ~m)
    return ~outside


def component(mask: np.ndarray, x: float, y: float) -> np.ndarray:
    """The 4-connected part of `mask` that holds pixel (x, y); empty when that pixel is off."""
    seed = np.zeros(mask.shape, bool)
    seed[int(y), int(x)] = True
    return flood(seed, mask)


def _taps(sigma: float) -> np.ndarray:
    r = max(1, int(np.ceil(3 * sigma)))
    k = np.exp(-0.5 * (np.arange(-r, r + 1) / sigma) ** 2)
    return k / k.sum()


def blur(img: np.ndarray, sigma: float) -> np.ndarray:
    """Separable Gaussian blur (edge-extended), on (H, W) or (H, W, C) float arrays."""
    a = np.asarray(img, np.float32)
    if sigma <= 0:
        return a.copy()
    k = _taps(sigma)
    r = len(k) // 2
    for axis in (0, 1):
        pad = [(0, 0)] * a.ndim
        pad[axis] = (r, r)
        p = np.pad(a, pad, mode="edge")
        n = a.shape[axis]
        acc = np.zeros_like(a)
        for i, wgt in enumerate(k):
            acc += wgt * np.take(p, np.arange(i, i + n), axis=axis)
        a = acc
    return a


def soften(mask: np.ndarray, grow: int = 0, feather: float = 0.0) -> np.ndarray:
    """A hard mask as a soft coverage: grown by `grow` px, then edge-blurred by `feather` px."""
    m = dilate(mask, grow) if grow else mask.astype(bool)
    f = m.astype(np.float32)
    if feather > 0:
        f = blur(f, feather)
    return np.clip(f, 0.0, 1.0)


def smoothstep(e0: float, e1: float, x: np.ndarray) -> np.ndarray:
    t = np.clip((np.asarray(x, np.float32) - e0) / max(1e-6, e1 - e0), 0.0, 1.0)
    return t * t * (3 - 2 * t)


def lum(rgb: np.ndarray) -> np.ndarray:
    """A pixel's brightest channel, the measure the renderer's light uses."""
    return np.asarray(rgb, np.float32).max(axis=2)


def bbox(alpha: np.ndarray, min_alpha: float, pad: int = 0) -> tuple[int, int, int, int] | None:
    """[x, y, w, h] around the pixels at or above `min_alpha`, grown by `pad` and kept on the canvas."""
    ys, xs = np.nonzero(alpha >= min_alpha)
    if len(xs) == 0:
        return None
    h, w = alpha.shape
    x0, y0 = max(0, int(xs.min()) - pad), max(0, int(ys.min()) - pad)
    x1, y1 = min(w, int(xs.max()) + 1 + pad), min(h, int(ys.max()) + 1 + pad)
    return x0, y0, x1 - x0, y1 - y0


def ring_fit(
    light: np.ndarray, cx: float, cy: float, r: float, search: int = 6
) -> tuple[float, float, float]:
    """Refine a bright ring's centre and radius: the circle whose median brightness is highest."""
    h, w = light.shape
    ang = np.linspace(0, 2 * np.pi, 360, endpoint=False)
    best = (-1.0, cx, cy, r)
    steps = range(-search, search + 1, 2)
    for dx in steps:
        for dy in steps:
            for dr in steps:
                xs = np.clip((cx + dx + (r + dr) * np.cos(ang)).astype(int), 0, w - 1)
                ys = np.clip((cy + dy + (r + dr) * np.sin(ang)).astype(int), 0, h - 1)
                s = float(np.median(light[ys, xs]))
                if s > best[0]:
                    best = (s, cx + dx, cy + dy, r + dr)
    return float(best[1]), float(best[2]), float(best[3])


def ring_mask(shape: tuple[int, int], cx: float, cy: float, r: float, width: float) -> np.ndarray:
    """The band of pixels within `width` px of the circle."""
    h, w = shape
    yy, xx = np.mgrid[0:h, 0:w]
    return np.abs(np.hypot(xx + 0.5 - cx, yy + 0.5 - cy) - r) < width


def channel_excess(rgb: np.ndarray, over: Sequence[int], under: Sequence[int]) -> np.ndarray:
    """Mean of the `over` channels minus the mean of the `under` ones: how much a pixel leans to a hue."""
    a = np.asarray(rgb, np.float32)
    return a[..., list(over)].mean(axis=2) - a[..., list(under)].mean(axis=2)
