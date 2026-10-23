# SPDX-License-Identifier: Apache-2.0
"""The jaw cut and its caps, on a drawn sphere: no model, no GPU, numpy only."""

from __future__ import annotations

import unittest

import numpy as np

from shadowrunr_avatar3d import geom


def uv_sphere(rings: int = 12, segs: int = 16) -> tuple[np.ndarray, np.ndarray]:
    """A closed, outward-facing sphere of radius 0.5."""
    v = [(0.0, 0.5, 0.0)]
    for i in range(1, rings):
        th = np.pi * i / rings
        for j in range(segs):
            ph = 2 * np.pi * j / segs
            v.append((0.5 * np.sin(th) * np.sin(ph), 0.5 * np.cos(th), 0.5 * np.sin(th) * np.cos(ph)))
    v.append((0.0, -0.5, 0.0))
    f = []
    ring = lambda i, j: 1 + (i - 1) * segs + j % segs  # noqa: E731
    for j in range(segs):
        f.append((0, ring(1, j), ring(1, j + 1)))
    for i in range(1, rings - 1):
        for j in range(segs):
            a, b, c, d = ring(i, j), ring(i, j + 1), ring(i + 1, j), ring(i + 1, j + 1)
            f += [(a, c, d), (a, d, b)]
    last = len(v) - 1
    for j in range(segs):
        f.append((last, ring(rings - 1, j + 1), ring(rings - 1, j)))
    return np.array(v), np.array(f)


def signed_volume(v: np.ndarray, f: np.ndarray) -> float:
    a, b, c = v[f[:, 0]], v[f[:, 1]], v[f[:, 2]]
    return float(np.einsum("ij,ij->i", a, np.cross(b, c)).sum() / 6)


class Sphere(unittest.TestCase):
    def test_drawn_sphere_is_closed_and_outward(self):
        v, f = uv_sphere()
        self.assertTrue(geom.is_closed(f))
        self.assertGreater(signed_volume(v, f), 0)


class JawCut(unittest.TestCase):
    def test_split_gives_two_closed_outward_pieces(self):
        v, f = uv_sphere()
        cent = v[f].mean(axis=1)
        jaw = (cent[:, 1] < -0.1) & (cent[:, 2] > 0.0)  # lower front: a "jaw"
        (sv, sf, scap), (jv, jf, jcap) = geom.split(v, f, jaw)
        for pv, pf, cap in ((sv, sf, scap), (jv, jf, jcap)):
            self.assertTrue(geom.is_closed(pf))
            self.assertGreater(signed_volume(pv, pf), 0)
            self.assertTrue(cap.any())
            self.assertFalse(cap[: len(pf) - cap.sum()].any())  # caps come last
        self.assertEqual(len(jf) - jcap.sum(), int(jaw.sum()))
        self.assertEqual(len(sf) - scap.sum(), int((~jaw).sum()))
        # The two pieces' volumes add up to the sphere's (the caps meet).
        self.assertAlmostEqual(signed_volume(sv, sf) + signed_volume(jv, jf), signed_volume(v, f), places=6)

    def test_two_holes_get_two_centres(self):
        v, f = uv_sphere()
        cent = v[f].mean(axis=1)
        keep = np.abs(cent[:, 1]) < 0.3  # a band: holes at the top and the bottom
        bv, bf, _ = geom.submesh(v, f, keep)
        cv, cf, cap = geom.cap_holes(bv, bf)
        self.assertTrue(geom.is_closed(cf))
        self.assertEqual(len(cv) - len(bv), 2)

    def test_closed_mesh_is_left_alone(self):
        v, f = uv_sphere()
        cv, cf, cap = geom.cap_holes(v, f)
        self.assertEqual(len(cf), len(f))
        self.assertFalse(cap.any())


class Helpers(unittest.TestCase):
    def test_point_in_poly(self):
        poly = [(0, 0), (10, 0), (10, 10), (5, 4), (0, 10)]  # a notch at the top
        pts = np.array([(5, 2), (5, 8), (1, 8), (-1, 5), (9.5, 9)])
        self.assertEqual(geom.point_in_poly(pts, poly).tolist(), [True, False, True, False, True])

    def test_floaters_are_dropped(self):
        v, f = uv_sphere()
        speck_v = v[:4] * 0.01 + 3.0
        speck_f = np.array([[0, 1, 2]]) + len(v)
        sv, sf, used = geom.remove_floaters(np.concatenate([v, speck_v]), np.concatenate([f, speck_f]))
        self.assertEqual(len(sf), len(f))
        self.assertLess(np.abs(sv).max(), 1.0)
        self.assertEqual(used.tolist(), list(range(len(v))))

    def test_normals_point_out(self):
        v, f = uv_sphere()
        n = geom.vertex_normals(v, f)
        self.assertGreater(np.einsum("ij,ij->i", n, v / np.linalg.norm(v, axis=1, keepdims=True)).min(), 0.9)


if __name__ == "__main__":
    unittest.main()
