# SPDX-License-Identifier: Apache-2.0
"""The collar and shoulders, cut from a generated bust.

``prep`` also writes ``input_bust.png`` (the rig's torso layer under the clean skull) and ``gen
--input bust`` turns it into a mesh of skull, high collar and cloak. This module:

1. orients the bust and fits the portrait's camera to the bust's matte (like the skull's);
2. maps it into the skull's frame: both cameras have the same field of view and look at the same
   portrait, so the two frames differ by a uniform scale about the camera targets (the ratio of the
   camera distances);
3. drops the bust's own skull, which is the bone-coloured part (the cloak is black, the trim is
   gold: saturated), then keeps the large connected pieces of what is left (the sockets' dark
   walls are islands in the skull and go with it);
4. caps the open rims (dark: the inside of cloth), decimates, unwraps its own atlas and paints it
   from the portrait where the skull's camera saw the surface, the model's colour elsewhere.

The collar is a root node ``collar`` whose translation is the neck anchor (see
:mod:`.contract`).
"""

from __future__ import annotations

import numpy as np

from . import fit, geom, prep, review
from .render import project

CAP_RGB = np.array([0.02, 0.02, 0.022])


def split_bone(c: np.ndarray, f: np.ndarray, v: np.ndarray | None = None,
               skull_v: np.ndarray | None = None, near: float = 0.045) -> np.ndarray:
    """A mask over faces: True where the face is the bust's own skull. By colour: pale and grey
    (the back of the head is grey, the cloak near black, the trim saturated gold). By place: a face
    within ``near`` of the final skull's dense surface is skull too, whatever its colour.
    ``c``: per-vertex colour in 0..1."""
    fc = c[f].mean(axis=1)
    mx, mn = fc.max(axis=1), fc.min(axis=1)
    lum = fc @ np.array([0.299, 0.587, 0.114])
    sat = (mx - mn) / np.maximum(mx, 1e-3)
    bone = (lum > 0.2) & (sat < 0.32)
    if v is not None and skull_v is not None:
        from scipy.spatial import cKDTree

        d, _ = cKDTree(skull_v[::3]).query(v[f].mean(axis=1))
        bone |= d < near
    return bone


def smooth_mask(f: np.ndarray, mask: np.ndarray, rounds: int = 2) -> np.ndarray:
    """Majority-vote a face mask over edge neighbours, to take the speckle out of a colour split."""
    e = np.sort(np.concatenate([f[:, [0, 1]], f[:, [1, 2]], f[:, [2, 0]]]), axis=1)
    fid = np.tile(np.arange(len(f)), 3)
    order = np.lexsort((e[:, 1], e[:, 0]))
    e, fid = e[order], fid[order]
    same = (e[1:] == e[:-1]).all(axis=1)
    a, b = fid[:-1][same], fid[1:][same]
    m = mask.astype(float)
    for _ in range(rounds):
        tot = np.zeros(len(f))
        cnt = np.zeros(len(f))
        np.add.at(tot, a, m[b])
        np.add.at(tot, b, m[a])
        np.add.at(cnt, a, 1)
        np.add.at(cnt, b, 1)
        m = np.where(cnt > 0, (tot + m) / (cnt + 1), m)
        m = (m > 0.5).astype(float)
    return m > 0.5


def keep_large(v: np.ndarray, f: np.ndarray, min_share: float = 0.04):
    """The connected pieces holding at least ``min_share`` of the faces (the cloak, the collar's
    wings), by shared vertex."""
    return geom.remove_floaters(v, f, min_share)


def cut(v: np.ndarray, f: np.ndarray, c: np.ndarray, skull_v: np.ndarray | None = None, log=print):
    """The bust's non-bone faces, the large pieces only. Returns (v, f, c)."""
    bone = smooth_mask(f, split_bone(c, f, v, skull_v))
    keep = ~bone
    sv, sf, used = geom.submesh(v, f, keep)
    sc = c[used]
    kv, kf, used2 = keep_large(sv, sf)
    log(f"collar: {int(keep.sum())} of {len(f)} bust faces are not bone; {len(kf)} in the large pieces")
    return kv, kf, sc[used2]


def make(a, r, skull_v, skull_f, view, proj, cam_pos, cam_target, size, portrait, log=print):
    """The collar for ``build``: a dict with ``v`` (skull frame), ``f``, ``uv``, ``png``, ``mime``
    and ``img`` (the atlas), or None when no usable bust is given."""
    from scipy.spatial import cKDTree

    from . import build as B

    if a.collar == "none" or not a.bust:
        return None
    cand = a.work / "cand" / a.bust
    vb, fb, cb, meta = review.oriented(cand, log)
    vb, fb, used = geom.remove_floaters(vb, fb)
    cb = cb[used]
    vb = fit.normalize(vb)
    from .render import Renderer

    pimg = prep.bust_layers(a.rig, (size, size))
    matte = pimg[..., 3].astype(np.float64) / 255
    rb = Renderer(size)
    bpos, btarget, bfov, biou = fit.fit_camera(rb, vb, fb, matte, log=log)
    dist_b = float(np.linalg.norm(np.asarray(bpos) - np.asarray(btarget)))
    dist_s = float(np.linalg.norm(np.asarray(cam_pos) - np.asarray(cam_target)))
    s = dist_s / dist_b
    log(f"collar: bust camera IoU {biou:.3f}, scale to the skull's frame {s:.3f}")
    vb = np.asarray(cam_target) + s * (vb - np.asarray(btarget))

    kv, kf, kc = cut(vb, fb, cb, skull_v, log)
    # Close the open rims first (dense), decimate the cloth, close again.
    dv, df = B.decimate(kv, kf, max(a.collar_budget - 300, 1000))
    dv, df, dcap = geom.cap_holes(dv, df)
    part = B.unwrap([{"v": dv, "f": df, "cap": dcap}], 2048)[0]
    log(f"collar: {len(part['f'])} tris after decimation and caps")

    # Paint: the portrait where the skull's camera sees this surface, the bust's colour elsewhere.
    tex_size = 2048
    pos, face = r.bake_positions(part["v"], part["f"], part["uv"], tex_size)
    m = face >= 0
    pts = pos[m].astype(np.float64)
    fidx = face[m]
    fn = np.cross(part["v"][part["f"][:, 1]] - part["v"][part["f"][:, 0]],
                  part["v"][part["f"][:, 2]] - part["v"][part["f"][:, 0]])
    fn /= np.maximum(np.linalg.norm(fn, axis=1, keepdims=True), 1e-12)
    n = fn[fidx]
    tree = cKDTree(kv)
    _, nn = tree.query(pts, k=4)
    model_c = kc[nn].mean(axis=1)
    # Visibility against the whole figure: skull and collar together.
    allv = np.concatenate([skull_v, part["v"]])
    allf = np.concatenate([skull_f, part["f"] + len(skull_v)])
    posmap, cover = r.positions(allv, allf, view, proj)
    q = project(pts, view, proj, size)
    xi = np.clip(q[:, 0].astype(int), 0, size - 1)
    yi = np.clip(q[:, 1].astype(int), 0, size - 1)
    seen = cover[yi, xi] & (np.linalg.norm(posmap[yi, xi] - pts, axis=1) < 0.012)
    eye = np.linalg.inv(view)[:3, 3]
    to_cam = eye - pts
    to_cam /= np.linalg.norm(to_cam, axis=1, keepdims=True)
    facing = np.abs(np.einsum("ij,ij->i", n, to_cam))
    w = np.clip((facing - 0.1) / 0.3, 0, 1) * seen
    # The portrait's own pixels (the torso layer's trim and cloth, not the skull's).
    port = portrait[yi, xi]
    # Where the portrait has the skull in front (jaw over the collar) the position test already
    # excludes the point; the rest is cloth and gold.
    c = w[:, None] * port + (1 - w[:, None]) * model_c
    img = np.zeros((tex_size, tex_size, 3))
    filled = np.zeros((tex_size, tex_size), bool)
    cap = part["cap"][fidx]
    c[cap] = CAP_RGB
    img[m] = np.clip(c, 0, 1)
    filled |= m
    img = B.dilate(img, filled)
    atlas = (np.clip(img, 0, 1) * 255).round().astype(np.uint8)
    png, mime = B.image_bytes(atlas, a.tex_format, a.jpeg_quality)
    log(f"collar: portrait paints {float(np.mean(w > 0.5)):.0%} of its texels; atlas {len(png) // 1024} KiB")
    return {"v": part["v"], "f": part["f"], "uv": part["uv"], "png": png, "mime": mime, "img": atlas}
