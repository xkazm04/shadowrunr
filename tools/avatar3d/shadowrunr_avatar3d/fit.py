# SPDX-License-Identifier: Apache-2.0
"""Put a generated mesh in the contract's frame and find the portrait's camera.

Every model has its own axes (Z-up or Y-up, facing +X or -Z...), so the orientation is fitted,
not assumed: the 24 axis-aligned rotations are each rendered from the front and scored against
the skull matte (silhouette overlap) and the skull picture (the dark sockets and nose make the
face side unmistakable, where a silhouette alone cannot tell the face from the back of the head).
The winner is refined by a few degrees of yaw, pitch and roll. Then a camera with a fixed field
of view is moved until the model's silhouette covers the portrait's skull.
"""

from __future__ import annotations

import itertools
import math

import numpy as np

from .render import Renderer, look_at, perspective

FOV = 20.0  # degrees, vertical: the portrait reads as a long lens
FIT = 192  # pixels: the size the orientation search renders and compares at


def rot(axis: str, deg: float) -> np.ndarray:
    c, s = math.cos(math.radians(deg)), math.sin(math.radians(deg))
    return {
        "x": np.array([[1, 0, 0], [0, c, -s], [0, s, c]]),
        "y": np.array([[c, 0, s], [0, 1, 0], [-s, 0, c]]),
        "z": np.array([[c, -s, 0], [s, c, 0], [0, 0, 1]]),
    }[axis]


def cube_rotations() -> list[np.ndarray]:
    """The 24 proper rotations that map axes onto axes."""
    out = []
    for perm in itertools.permutations(range(3)):
        for signs in itertools.product((1, -1), repeat=3):
            m = np.zeros((3, 3))
            for i, (p, s) in enumerate(zip(perm, signs)):
                m[i, p] = s
            if np.linalg.det(m) > 0:
                out.append(m)
    return out


def normalize(v: np.ndarray) -> np.ndarray:
    """Centre the bounding box on the origin and scale to one unit tall."""
    lo, hi = v.min(axis=0), v.max(axis=0)
    return (v - (lo + hi) / 2) / (hi[1] - lo[1])


def fit_square(img: np.ndarray, size: int = FIT) -> np.ndarray:
    """Crop to the content's bbox and fit it, aspect kept, in a centred square."""
    from PIL import Image

    a = img if img.ndim == 2 else img[..., -1]
    ys, xs = np.nonzero(a > 0.5 * a.max())
    crop = img[ys.min() : ys.max() + 1, xs.min() : xs.max() + 1]
    h, w = crop.shape[:2]
    k = (size - 4) / max(h, w)
    nh, nw = max(1, round(h * k)), max(1, round(w * k))
    mode = "F"
    chans = [crop] if crop.ndim == 2 else [crop[..., i] for i in range(crop.shape[2])]
    out = np.zeros((size, size, len(chans)), np.float32)
    y0, x0 = (size - nh) // 2, (size - nw) // 2
    for i, ch in enumerate(chans):
        out[y0 : y0 + nh, x0 : x0 + nw, i] = np.asarray(
            Image.fromarray(ch.astype(np.float32), mode).resize((nw, nh), Image.BILINEAR)
        )
    return out[..., 0] if img.ndim == 2 else out


def front_view(dist: float = 4.0):
    view = look_at((0, 0, dist), (0, 0, 0))
    return view, perspective(FOV)


def score_view(r: Renderer, v, f, c, ref_rgba: np.ndarray) -> float:
    """Silhouette IoU plus the luminance correlation inside the overlap, both bbox-normalised."""
    view, proj = front_view()
    rgb, a = r.draw([{"v": v, "f": f, "c": c}], view, proj, eye=(0, 0, 4), lit=0.0, ssaa=1)
    if a.max() < 0.5:
        return -1.0
    got = fit_square(np.dstack([rgb.astype(np.float32) / 255, a]))
    ref = fit_square(ref_rgba.astype(np.float32) / 255)
    ma, mb = got[..., 3] > 0.5, ref[..., 3] > 0.5
    iou = (ma & mb).sum() / max((ma | mb).sum(), 1)
    both = ma & mb
    la, lb = got[..., :3].mean(-1)[both], ref[..., :3].mean(-1)[both]
    corr = float(np.corrcoef(la, lb)[0, 1]) if both.sum() > 16 and la.std() > 1e-4 else 0.0
    return float(iou + 0.5 * corr)


def orient(r: Renderer, v, f, c, ref_rgba: np.ndarray, log=print) -> tuple[np.ndarray, float]:
    """The rotation that turns the mesh to face +Z, up +Y, matching the prepped skull. Render
    with a small renderer (``FIT`` or so): this draws the mesh some 150 times."""
    best = (-9.0, np.eye(3))
    for m in cube_rotations():
        s = score_view(r, normalize(v @ m.T), f, c, ref_rgba)
        if s > best[0]:
            best = (s, m)
    log(f"orient: best axis rotation scores {best[0]:.3f}")
    for step in (6.0, 2.0):
        improved = True
        while improved:
            improved = False
            for axis, sign in itertools.product("xyz", (1, -1)):
                m = rot(axis, sign * step) @ best[1]
                s = score_view(r, normalize(v @ m.T), f, c, ref_rgba)
                if s > best[0] + 1e-4:
                    best, improved = (s, m), True
    log(f"orient: refined to {best[0]:.3f}")
    return best[1], best[0]


def fit_camera(r: Renderer, v, f, matte: np.ndarray, fov: float = FOV, log=print):
    """A camera on +Z looking down -Z whose render of the mesh covers ``matte`` (the portrait's
    skull coverage, ``r.size`` square). Returns (pos, target, fov, iou)."""
    size = r.size
    ys, xs = np.nonzero(matte > 0.5)
    ref_c = np.array([(xs.min() + xs.max() + 1) / 2, (ys.min() + ys.max() + 1) / 2])
    ref_h = ys.max() + 1 - ys.min()
    target = np.zeros(3)
    dist = 0.5 / math.tan(math.radians(fov) / 2) * size / ref_h
    proj = perspective(fov)

    def render(target, dist):
        view = look_at(target + np.array([0, 0, dist]), target)
        return r.mask(v, f, view, proj)

    for _ in range(6):
        a = render(target, dist)
        ys2, xs2 = np.nonzero(a > 0.5)
        h = ys2.max() + 1 - ys2.min()
        c = np.array([(xs2.min() + xs2.max() + 1) / 2, (ys2.min() + ys2.max() + 1) / 2])
        dist *= h / ref_h
        # Pixel offset to model units at the target's depth.
        units = 2 * dist * math.tan(math.radians(fov) / 2) / size
        target = target + np.array([-(ref_c[0] - c[0]) * units, (ref_c[1] - c[1]) * units, 0])
    ref = matte > 0.5

    def iou(target, dist):
        m = render(target, dist) > 0.5
        return (m & ref).sum() / max((m | ref).sum(), 1)

    best = (iou(target, dist), target, dist)
    for step in (0.01, 0.004):
        improved = True
        while improved:
            improved = False
            for d in ((step, 0, 0), (-step, 0, 0), (0, step, 0), (0, -step, 0)):
                t2 = best[1] + np.array(d)
                s = iou(t2, best[2])
                if s > best[0] + 1e-5:
                    best, improved = (s, t2, best[2]), True
            for k in (1 + step, 1 - step):
                s = iou(best[1], best[2] * k)
                if s > best[0] + 1e-5:
                    best, improved = (s, best[1], best[2] * k), True
    s, target, dist = best
    log(f"camera: fov {fov} deg, distance {dist:.3f}, silhouette IoU {s:.3f}")
    return target + np.array([0, 0, dist]), target, fov, float(s)
