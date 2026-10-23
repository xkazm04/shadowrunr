# SPDX-License-Identifier: Apache-2.0
"""Texture synthesis and baking helpers that need numpy only (scipy for two of them), so the
maths is unit-tested without a model, a GPU or the portrait.

- **Unseen bone.** The portrait shows the skull from the front. Everything the camera did not see
  (the cranium's sides and back, the underside) gets bone cut from the front: a swatch of lit
  bone is high-passed into a multiplicative detail map (cracks, grain, stains at the front's own
  scale), then looked up in object space by a triplanar projection. Each lookup blends three
  decorrelated copies of the swatch with a variance-preserving mix, so the texture neither
  repeats visibly nor washes out to flat beige.
- **Tangent-space normals.** The decimated mesh's normal map is the dense mesh's normal at each
  texel, expressed in the decimated mesh's tangent frame (glTF convention: +X along ``u``, +Y along
  decreasing glTF ``v``, +Z out; the frame renderers rebuild from the UVs when a mesh has no
  tangents).
- **Cavity.** A curvature term from the dense mesh (the neighbours' mean lies above a concave
  point, below a convex one) that darkens creases and lifts edges a little.
"""

from __future__ import annotations

import numpy as np

# --- the swatch ------------------------------------------------------------------------------


def box_blur(img: np.ndarray, r: int) -> np.ndarray:
    """Mean over a (2r+1) square, edge-clamped; numpy only."""
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


def detail_map(swatch: np.ndarray, radius: int = 18) -> np.ndarray:
    """A swatch of lit bone (h, w, 3 in 0..1) as multiplicative detail around 1: the swatch over
    its own blur, three box passes deep (a close match for a Gaussian), the ratio clamped."""
    s = swatch.astype(np.float64)
    low = s
    for _ in range(3):
        low = box_blur(low, radius)
    return np.clip(s / np.maximum(low, 0.02), 0.3, 2.5)


def _noise(p: np.ndarray, seed: int, freq: float) -> np.ndarray:
    """A smooth 0..1 field of 3D position: three sines at irrational frequencies (no lattice)."""
    rng = np.random.default_rng(seed)
    out = np.zeros(len(p))
    for _ in range(3):
        d = rng.normal(size=3)
        d /= np.linalg.norm(d)
        out += np.sin(freq * (p @ d) * rng.uniform(0.8, 1.3) + rng.uniform(0, 6.28))
    return 0.5 + out / 6.0


def _lookup(detail: np.ndarray, u: np.ndarray, v: np.ndarray) -> np.ndarray:
    h, w = detail.shape[:2]
    return detail[np.floor(v).astype(np.int64) % h, np.floor(u).astype(np.int64) % w]


def synth_detail(detail: np.ndarray, pos: np.ndarray, normal: np.ndarray, px_per_unit: float,
                 seed: int = 7) -> np.ndarray:
    """Detail (around 1, (n, 3)) for surface points: triplanar projection of ``detail`` (a
    swatch's :func:`detail_map`) at ``px_per_unit`` swatch pixels per scene unit. Per plane two
    copies of the swatch (offset, one transposed) are mixed by a smooth field; the three planes by
    the normal's power. Every mix keeps the variance (weights divided by their L2 norm)."""
    h, w = detail.shape[:2]
    p = pos * px_per_unit
    wts = np.abs(normal) ** 4
    wts = wts / np.maximum(wts.sum(axis=1, keepdims=True), 1e-9)
    acc = np.zeros((len(pos), 3))
    norm = np.zeros(len(pos))
    for axis in range(3):
        a, b = [i for i in range(3) if i != axis]
        u, v = p[:, a], p[:, b]
        m = _noise(pos, seed + axis, 9.0)
        c0 = _lookup(detail, u, v) - 1.0
        c1 = _lookup(detail, v + 0.37 * w, u + 0.61 * h) - 1.0  # transposed and shifted
        k = np.sqrt(m**2 + (1 - m) ** 2)
        mix = (m[:, None] * c0 + (1 - m)[:, None] * c1) / k[:, None]
        acc += wts[:, axis : axis + 1] * mix
        norm += wts[:, axis] ** 2
    acc /= np.sqrt(np.maximum(norm, 1e-9))[:, None]
    return 1.0 + acc


# --- normals ---------------------------------------------------------------------------------


def face_tangents(v: np.ndarray, f: np.ndarray, uv: np.ndarray) -> tuple[np.ndarray, np.ndarray]:
    """Per face: the direction in which ``u`` grows (T) and the one in which the *flipped* v
    (``1 - v``, image-up) grows (B), unit length. UVs are glTF's (v grows downward)."""
    p0, p1, p2 = v[f[:, 0]], v[f[:, 1]], v[f[:, 2]]
    e1, e2 = p1 - p0, p2 - p0
    d1 = uv[f[:, 1]] - uv[f[:, 0]]
    d2 = uv[f[:, 2]] - uv[f[:, 0]]
    det = d1[:, 0] * d2[:, 1] - d2[:, 0] * d1[:, 1]
    det = np.where(np.abs(det) < 1e-12, 1e-12, det)
    t = (e1 * d2[:, 1:2] - e2 * d1[:, 1:2]) / det[:, None]
    b = -(e2 * d1[:, 0:1] - e1 * d2[:, 0:1]) / det[:, None]
    nt = np.linalg.norm(t, axis=1, keepdims=True)
    nb = np.linalg.norm(b, axis=1, keepdims=True)
    return t / np.maximum(nt, 1e-12), b / np.maximum(nb, 1e-12)


def barycentric(p: np.ndarray, a: np.ndarray, b: np.ndarray, c: np.ndarray) -> np.ndarray:
    """Barycentric weights (n, 3) of points ``p`` in triangles (a, b, c), each (n, 3)."""
    v0, v1, v2 = b - a, c - a, p - a
    d00 = np.einsum("ij,ij->i", v0, v0)
    d01 = np.einsum("ij,ij->i", v0, v1)
    d11 = np.einsum("ij,ij->i", v1, v1)
    d20 = np.einsum("ij,ij->i", v2, v0)
    d21 = np.einsum("ij,ij->i", v2, v1)
    den = np.where(np.abs(d00 * d11 - d01 * d01) < 1e-18, 1e-18, d00 * d11 - d01 * d01)
    y = (d11 * d20 - d01 * d21) / den
    z = (d00 * d21 - d01 * d20) / den
    return np.stack([1 - y - z, y, z], axis=1)


def to_tangent_space(n_world: np.ndarray, n_surf: np.ndarray, t: np.ndarray, b: np.ndarray) -> np.ndarray:
    """``n_world`` in the frame (T, B, N) built from the surface normal ``n_surf`` and the face's
    T and B (Gram-Schmidt on N; handedness from B). All (n, 3), unit."""
    nn = n_surf / np.maximum(np.linalg.norm(n_surf, axis=1, keepdims=True), 1e-12)
    tt = t - nn * np.einsum("ij,ij->i", nn, t)[:, None]
    tt /= np.maximum(np.linalg.norm(tt, axis=1, keepdims=True), 1e-12)
    bb = np.cross(nn, tt)
    sign = np.sign(np.einsum("ij,ij->i", bb, b))
    bb *= np.where(sign == 0, 1.0, sign)[:, None]
    out = np.stack([np.einsum("ij,ij->i", n_world, tt), np.einsum("ij,ij->i", n_world, bb),
                    np.einsum("ij,ij->i", n_world, nn)], axis=1)
    return out / np.maximum(np.linalg.norm(out, axis=1, keepdims=True), 1e-12)


def encode_normals(n: np.ndarray) -> np.ndarray:
    """Unit vectors in -1..1 to the RGB8 a normal map stores."""
    return np.clip((n * 0.5 + 0.5) * 255.0 + 0.5, 0, 255).astype(np.uint8)


def decode_normals(rgb: np.ndarray) -> np.ndarray:
    n = rgb.astype(np.float64) / 255.0 * 2 - 1
    return n / np.maximum(np.linalg.norm(n, axis=-1, keepdims=True), 1e-12)


# --- cavity ----------------------------------------------------------------------------------


def cavity(v: np.ndarray, n: np.ndarray, k: int = 10, subsample: int = 0, seed: int = 0) -> np.ndarray:
    """Signed curvature proxy per vertex: positive in a crease (the neighbours' mean sits above
    the point along its normal), negative on an edge, scaled by the neighbourhood's radius so
    the value reads as a slope. ``subsample`` > 0 builds the neighbour tree from that many random
    vertices instead of all (a wider radius for the same ``k``)."""
    from scipy.spatial import cKDTree

    if subsample and subsample < len(v):
        ref = v[np.random.default_rng(seed).choice(len(v), subsample, replace=False)]
    else:
        ref = v
    tree = cKDTree(ref)
    d, idx = tree.query(v, k=k + 1)
    mean = ref[idx[:, 1:]].mean(axis=1)
    r = np.maximum(d[:, 1:].mean(axis=1), 1e-9)
    return np.einsum("ij,ij->i", mean - v, n) / r


def cavity_factor(c_fine: np.ndarray, c_wide: np.ndarray, strength: float) -> np.ndarray:
    """A multiplier around 1 (creases darker, edges a touch lighter) from the two scales, each
    normalised by its own spread so ``strength`` means the same on every mesh."""
    out = np.zeros(len(c_fine))
    for c, wt in ((c_fine, 0.5), (c_wide, 0.5)):
        s = np.percentile(np.abs(c), 90) + 1e-9
        out += wt * np.clip(c / s, -1.0, 1.0)
    return np.clip(1.0 - strength * np.where(out > 0, out, 0.35 * out), 0.5, 1.15)
