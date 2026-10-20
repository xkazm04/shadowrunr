# SPDX-License-Identifier: Apache-2.0
"""Mask and region maths (numpy). Skipped where numpy is not installed."""

from __future__ import annotations

import unittest

try:
    import numpy as np

    from shadowrunr_rig import masks as mk
except ImportError:  # pragma: no cover - the interpreter has no numpy
    np = None


@unittest.skipIf(np is None, "numpy is not installed")
class Masks(unittest.TestCase):
    def test_a_polygon_covers_the_pixels_whose_centres_are_inside(self):
        m = mk.poly_mask((20, 20), [[2, 3], [12, 3], [12, 9], [2, 9]])
        self.assertEqual(int(m.sum()), 10 * 6)
        self.assertTrue(m[3, 2] and m[8, 11])
        self.assertFalse(m[2, 2] or m[9, 2] or m[3, 12])
        tri = mk.poly_mask((40, 40), [[0, 0], [40, 0], [0, 40]])
        self.assertAlmostEqual(int(tri.sum()), 800, delta=40)
        # winding does not matter
        self.assertTrue(
            np.array_equal(m, mk.poly_mask((20, 20), [[2, 9], [12, 9], [12, 3], [2, 3]]))
        )

    def test_mirror_reflects_about_the_axis(self):
        self.assertEqual(mk.mirror_points([[10, 4], [30, 6]], 50), [[90.0, 4.0], [70.0, 6.0]])

    def test_dilate_and_erode_grow_and_shrink_about_r(self):
        m = np.zeros((41, 41), bool)
        m[20, 20] = True
        d = mk.dilate(m, 4)
        ys, xs = np.nonzero(d)
        self.assertEqual((xs.min(), xs.max(), ys.min(), ys.max()), (16, 24, 16, 24))
        self.assertFalse(d[16, 16])  # an octagon, not a square
        self.assertTrue(np.array_equal(mk.erode(d, 4), m))
        block = np.ones((10, 10), bool)
        self.assertEqual(int(mk.erode(block, 1).sum()), 64)  # the canvas edge is outside

    def test_close_bridges_a_narrow_gap_and_keeps_a_wide_one(self):
        m = np.zeros((30, 40), bool)
        m[10:20, 5:15] = True
        m[10:20, 17:27] = True  # a 2 px gap
        self.assertTrue(mk.close(m, 2)[15, 16])
        m2 = np.zeros((30, 40), bool)
        m2[10:20, 2:12] = True
        m2[10:20, 24:34] = True  # a 12 px gap
        self.assertFalse(mk.close(m2, 2)[15, 18])

    def test_fill_holes_fills_what_the_border_cannot_reach(self):
        ring = mk.ring_mask((40, 40), 20, 20, 10, 2)
        filled = mk.fill_holes(ring)
        self.assertTrue(filled[20, 20])
        self.assertFalse(filled[2, 2])
        cup = np.zeros((20, 20), bool)
        cup[5:15, 5] = cup[5:15, 14] = cup[14, 5:15] = True  # open at the top
        self.assertFalse(mk.fill_holes(cup)[10, 10])

    def test_component_is_the_connected_part_under_the_seed(self):
        m = np.zeros((10, 20), bool)
        m[2:8, 2:6] = True
        m[2:8, 10:14] = True
        c = mk.component(m, 3, 4)
        self.assertEqual(int(c.sum()), 24)
        self.assertFalse(c[4, 11])
        self.assertEqual(int(mk.component(m, 8, 4).sum()), 0)

    def test_blur_and_soften_keep_the_mass_and_soften_the_edge(self):
        m = np.zeros((40, 40), bool)
        m[10:30, 10:30] = True
        s = mk.soften(m, 0, 2.0)
        self.assertAlmostEqual(float(s.sum()), 400.0, delta=1.0)
        self.assertGreater(s[20, 9], 0.0)
        self.assertLess(s[20, 9], 0.5)
        self.assertAlmostEqual(float(s[20, 20]), 1.0, places=4)
        self.assertEqual(mk.soften(m).dtype, np.float32)

    def test_bbox_pads_and_stays_on_the_canvas(self):
        a = np.zeros((50, 60), np.float32)
        a[10:20, 30:40] = 255
        self.assertEqual(mk.bbox(a, 3), (30, 10, 10, 10))
        self.assertEqual(mk.bbox(a, 3, pad=15), (15, 0, 40, 35))
        self.assertIsNone(mk.bbox(np.zeros((4, 4)), 3))

    def test_ring_fit_finds_a_bright_ring_near_the_guess(self):
        yy, xx = np.mgrid[0:120, 0:120]
        d = np.hypot(xx + 0.5 - 62, yy + 0.5 - 58) - 30
        light = (250 * np.exp(-((d / 1.5) ** 2))).astype(
            np.float32
        )  # a thin ring, brightest on its line
        cx, cy, r = mk.ring_fit(light, 60, 60, 32)
        self.assertLessEqual(abs(cx - 62), 1)
        self.assertLessEqual(abs(cy - 58), 1)
        self.assertLessEqual(abs(r - 30), 1)

    def test_channel_excess_measures_a_lean_to_teal(self):
        px = np.array([[[40, 200, 180], [200, 180, 150], [5, 5, 5]]], np.float32)
        lean = mk.channel_excess(px, [1, 2], [0])
        self.assertEqual(lean.tolist(), [[150.0, -35.0, 0.0]])
        self.assertEqual(mk.smoothstep(6, 30, lean).tolist(), [[1.0, 0.0, 0.0]])


if __name__ == "__main__":
    unittest.main()
