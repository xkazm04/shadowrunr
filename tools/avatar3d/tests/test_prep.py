# SPDX-License-Identifier: Apache-2.0
import unittest

import numpy as np

from shadowrunr_avatar3d import prep


class DesmearTest(unittest.TestCase):
    def scene(self):
        img = np.zeros((64, 64, 4), np.uint8)
        img[..., 3] = 255
        img[:, :32, :3] = (202, 184, 158)  # bone
        img[:, 32:48, :3] = (230, 120, 30)  # orange smear
        img[:, 48:, :3] = (4, 20, 20)  # dark socket (teal-black)
        return img

    def test_orange_becomes_bone_tinted(self):
        out, w = prep.desmear(self.scene())
        self.assertGreater(w[32, 38], 0.95)
        px = out[32, 40, :3].astype(float)
        self.assertLess(abs(px[0] / px[2] - 202 / 158), 0.12)  # red:blue like bone, not like orange

    def test_bone_and_dark_untouched(self):
        img = self.scene()
        out, w = prep.desmear(img)
        self.assertLess(w[32, 5], 0.01)
        self.assertLess(w[32, 60], 0.01)
        self.assertTrue((out[:, :24] == img[:, :24]).all())
        self.assertTrue((out[:, 56:] == img[:, 56:]).all())

    def test_alpha_kept(self):
        img = self.scene()
        img[:5, :, 3] = 0
        out, _ = prep.desmear(img)
        self.assertTrue((out[..., 3] == img[..., 3]).all())

    def test_box_is_a_mean(self):
        x = np.arange(25, dtype=float).reshape(5, 5)
        y = prep._box(x, 1)
        self.assertAlmostEqual(y[2, 2], x[1:4, 1:4].mean())


if __name__ == "__main__":
    unittest.main()
