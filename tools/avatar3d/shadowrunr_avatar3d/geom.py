# SPDX-License-Identifier: Apache-2.0
"""Mesh geometry the build needs, numpy only: the jaw cut, its caps, normals and polygon tests.

Meshes are (vertices (n, 3) float, faces (m, 3) int) with counter-clockwise faces seen from
outside. Everything here is deterministic and model-free, so it is unit-tested on drawn meshes.
"""

from __future__ import annotations

import numpy as np


def point_in_poly(pts: np.ndarray, poly: np.ndarray) -> np.ndarray:
    """Even-odd test of 2D points against one polygon (closed implicitly)."""
    pts = np.asarray(pts, float)
    poly = np.asarray(poly, float)
    x, y = pts[:, 0], pts[:, 1]
    inside = np.zeros(len(pts), bool)
    j = len(poly) - 1
    for i in range(len(poly)):
        xi, yi = poly[i]
        xj, yj = poly[j]
        cross = (yi > y) != (yj > y)
        with np.errstate(divide="ignore", invalid="ignore"):
            xc = (xj - xi) * (y - yi) / (yj - yi) + xi
        inside ^= cross & (x < xc)
        j = i
    return inside


def submesh(v: np.ndarray, f: np.ndarray, keep: np.ndarray) -> tuple[np.ndarray, np.ndarray, np.ndarray]:
    """The faces where ``keep`` holds, with only the vertices they use. Also returns, per new
    vertex, its index in ``v``."""
    ff = f[keep]
    used, inv = np.unique(ff.reshape(-1), return_inverse=True)
    return v[used], inv.reshape(-1, 3), used


def boundary_edges(f: np.ndarray) -> np.ndarray:
    """Directed edges (a, b) whose twin (b, a) no face has: the rim of every hole."""
    e = np.concatenate([f[:, [0, 1]], f[:, [1, 2]], f[:, [2, 0]]])
    n = int(f.max()) + 1 if f.size else 0
    key = e[:, 0].astype(np.int64) * n + e[:, 1]
    twin = e[:, 1].astype(np.int64) * n + e[:, 0]
    return e[~np.isin(twin, key)]


def _components(pairs: np.ndarray, n: int) -> np.ndarray:
    parent = np.arange(n)

    def find(a: int) -> int:
        while parent[a] != a:
            parent[a] = parent[parent[a]]
            a = parent[a]
        return a

    for a, b in pairs:
        ra, rb = find(int(a)), find(int(b))
        if ra != rb:
            parent[ra] = rb
    return np.array([find(i) for i in range(n)])


def cap_holes(v: np.ndarray, f: np.ndarray) -> tuple[np.ndarray, np.ndarray, np.ndarray]:
    """Close every hole with a fan to its rim's centroid. Returns the new vertices, faces and a
    mask over the new faces marking the caps (the build paints them dark: the inside of bone)."""
    be = boundary_edges(f)
    if len(be) == 0:
        return v, f, np.zeros(len(f), bool)
    comp = _components(be, len(v))
    rims = comp[be[:, 0]]
    ids, which = np.unique(rims, return_inverse=True)
    centres = np.zeros((len(ids), 3))
    for k in range(len(ids)):
        verts = np.unique(be[which == k].reshape(-1))
        centres[k] = v[verts].mean(axis=0)
    c_idx = len(v) + which
    caps = np.stack([be[:, 1], be[:, 0], c_idx], axis=1)
    v2 = np.concatenate([v, centres])
    f2 = np.concatenate([f, caps])
    mask = np.zeros(len(f2), bool)
    mask[len(f):] = True
    return v2, f2, mask


def is_closed(f: np.ndarray) -> bool:
    return len(boundary_edges(f)) == 0


def split(v: np.ndarray, f: np.ndarray, part: np.ndarray):
    """Cut a mesh in two along a face mask; each piece is capped. Returns, for the faces where
    ``part`` is false and where it is true: (vertices, faces, cap mask)."""
    out = []
    for keep in (~part, part):
        sv, sf, _ = submesh(v, f, keep)
        out.append(cap_holes(sv, sf))
    return out[0], out[1]


def vertex_normals(v: np.ndarray, f: np.ndarray) -> np.ndarray:
    fn = np.cross(v[f[:, 1]] - v[f[:, 0]], v[f[:, 2]] - v[f[:, 0]])
    n = np.zeros_like(v, dtype=float)
    for k in range(3):
        np.add.at(n, f[:, k], fn)
    ln = np.linalg.norm(n, axis=1, keepdims=True)
    return (n / np.where(ln > 0, ln, 1)).astype(np.float32)


def remove_floaters(v: np.ndarray, f: np.ndarray, min_share: float = 0.01):
    """Drop connected pieces with fewer than ``min_share`` of the faces (stray voxels). Also
    returns, per kept vertex, its index in ``v`` (to carry vertex colours along)."""
    pairs = np.concatenate([f[:, [0, 1]], f[:, [1, 2]]])
    comp = _components(pairs, len(v))[f[:, 0]]
    ids, counts = np.unique(comp, return_counts=True)
    keep = np.isin(comp, ids[counts >= min_share * len(f)])
    return submesh(v, f, keep)
