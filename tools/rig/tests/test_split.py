# SPDX-License-Identifier: Apache-2.0
"""The split end to end on a drawn portrait, with a stand-in inpainter (no model, no torch)."""

from __future__ import annotations

import unittest

try:
    import numpy as np

    from shadowrunr_rig import formspec as fsm
    from shadowrunr_rig import masks as mk
    from shadowrunr_rig.split import assemble, compose_rest, split
except ImportError:  # pragma: no cover - the interpreter has no numpy
    np = None

W = H = 128
GROUND = [1, 1, 1]
BONE = [200, 190, 170]


def portrait():
    """A fictional bust: a bone skull with a teal eye, a jaw, an ember spark, a floating shard."""
    img = np.full((H, W, 3), GROUND, np.float32)
    img[mk.poly_mask((H, W), [[40, 70], [88, 70], [100, 128], [28, 128]])] = [20, 20, 20]  # robe
    img[mk.poly_mask((H, W), [[44, 20], [84, 20], [88, 60], [40, 60]])] = BONE  # cranium
    img[mk.poly_mask((H, W), [[46, 60], [82, 60], [78, 76], [50, 76]])] = [190, 180, 160]  # jaw
    img[mk.poly_mask((H, W), [[50, 32], [60, 32], [60, 42], [50, 42]])] = [4, 10, 9]  # socket
    yy, xx = np.mgrid[0:H, 0:W]
    glow = np.exp(-(((xx + 0.5 - 55) / 3.0) ** 2 + ((yy + 0.5 - 37) / 3.0) ** 2))
    img[..., 1] += 180 * glow * (img[..., 1] < 20)
    img[..., 2] += 160 * glow * (img[..., 1] < 200)
    img[mk.poly_mask((H, W), [[100, 10], [106, 10], [106, 16], [100, 16]])] = [
        240,
        140,
        40,
    ]  # spark
    img[mk.poly_mask((H, W), [[20, 8], [26, 4], [32, 8], [26, 20]])] = [70, 75, 80]  # shard
    return np.clip(img, 0, 255)


def spec():
    return {
        "rig_type": "glow",
        "portrait": "test",
        "width": W,
        "height": H,
        "ground_rgb": GROUND,
        "head": {"center": [64, 44], "radius": [26, 34], "pivot_y": 74, "chin_y": 76},
        "aperture": {"seam_y": 60, "top": 50, "bottom": 120, "x0": 50, "x1": 78, "open_frac": 0.1},
        "colors": {"mouth": [0.2, 0.85, 0.78]},
        "elements": [
            {
                "name": "spark",
                "kind": "pulse",
                "mode": "light",
                "fill": "inpaint",
                "region": {"poly": [[96, 6], [110, 6], [110, 20], [96, 20]]},
                "pulse": {"amp": 0.1, "freq": 0.1},
            },
            {
                "name": "eye",
                "kind": "eye",
                "mode": "light",
                "fill": "key",
                "blink": "dim",
                "depth": "surface",
                "key": {"base": [4, 10, 9]},
                "region": {"poly": [[49, 31], [61, 31], [61, 43], [49, 43]]},
            },
            {
                "name": "shard",
                "kind": "float",
                "mode": "cut",
                "fill": "ground",
                "region": {"poly": [[16, 2], [36, 2], [36, 24], [16, 24]], "lit": 4, "holes": True},
                "float": {"amp": [1, 2], "freq": 0.2, "rot_deg": 2},
            },
            {
                "name": "jaw",
                "kind": "aperture_lower",
                "mode": "cut",
                "fill": "keep",
                "depth": "surface",
                "region": {"poly": [[44, 60], [84, 60], [84, 80], [44, 80]], "lit": 40},
                "interior": {"poly": [[46, 60], [82, 60], [82, 68], [46, 68]], "color": [1, 2, 2]},
                "uncover": 6,
            },
            {
                "name": "mouth_glow",
                "kind": "aperture_glow",
                "mode": "none",
                "depth": "surface",
                "depth_at": [64, 62],
                "gain": 0.2,
            },
            {
                "name": "head",
                "kind": "head",
                "mode": "cut",
                "fill": "ground",
                "region": {
                    "poly": [[38, 16], [90, 16], [90, 82], [38, 82]],
                    "lit": 40,
                    "holes": True,
                    "also": [[[46, 60], [82, 60], [82, 68], [46, 68]]],
                },
            },
            {
                "name": "torso",
                "kind": "body",
                "mode": "cut",
                "fill": "ground",
                "region": {"poly": [[30, 66], [98, 66], [104, 128], [24, 128]]},
            },
        ],
    }


def stand_in_fill(rgb, mask):
    """What an inpainter does here, without one: the mask's surroundings, averaged."""
    out = np.asarray(rgb, np.float32).copy()
    ring = mk.dilate(mask > 0, 2) & ~(mask > 0)
    out[mask > 0] = out[ring].mean(axis=0) if ring.any() else 0
    return out


@unittest.skipIf(np is None, "numpy is not installed")
class Split(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.img = portrait()
        cls.spec = spec()
        cls.result = split(cls.img, cls.spec, stand_in_fill)
        cls.fs, cls.pictures = assemble(cls.spec, cls.result, "test")
        cls.by = {layer["name"]: layer for layer in cls.fs["layers"]}

    def test_layers_are_written_back_to_front_and_check(self):
        names = [layer["name"] for layer in self.fs["layers"]]
        self.assertEqual(
            names, ["background", "torso", "head", "mouth_glow", "jaw", "shard", "eye", "spark"]
        )
        self.assertEqual([layer["z"] for layer in self.fs["layers"]], list(range(1, 9)))
        fsm.check_formspec(self.fs)
        self.assertEqual(self.fs["rig"]["colors"], {"mouth": [0.2, 0.85, 0.78]})

    def test_every_picture_is_its_bbox_and_named_once(self):
        files = [layer["file"] for layer in self.fs["layers"] if "file" in layer]
        self.assertEqual(sorted(files), sorted(self.pictures))
        for layer in self.fs["layers"]:
            if "file" in layer:
                x, y, w, h = layer["bbox"]
                self.assertEqual(self.pictures[layer["file"]].shape, (h, w, 4), layer["name"])
                self.assertEqual(self.pictures[layer["file"]].dtype, np.uint8)
        self.assertNotIn("file", self.by["mouth_glow"])
        self.assertNotIn("bbox", self.by["mouth_glow"])

    def test_the_rest_frame_recomposes_the_portrait(self):
        rest = compose_rest(self.fs, self.pictures)
        err = np.abs(rest - self.img).max(axis=2)
        self.assertLess(float(err.mean()), 1.0)
        self.assertLess(float(np.percentile(err, 99)), 12.0)

    def test_a_light_is_only_the_light_and_has_room_to_fade(self):
        eye = self.pictures["eye.png"].astype(np.float32)
        lean = mk.channel_excess(eye[..., :3], [1, 2], [0])
        self.assertGreater(float(lean.max()), 100)  # the teal glow went to the light layer
        bg = self.result.background
        self.assertLess(
            float(mk.channel_excess(bg[34:40, 52:58], [1, 2], [0]).max()), 20
        )  # and left the socket dark
        x, y, w, h = self.by["eye"]["bbox"]
        self.assertLessEqual(x, 49 - 2)
        self.assertGreaterEqual(w, 12 + 2 * 2)
        spark = self.pictures["spark.png"]
        self.assertGreater(int(spark[..., 0].max()), 200)

    def test_the_jaw_leaves_a_dark_mouth_and_nothing_else_behind_it(self):
        head = np.zeros((H, W, 4), np.float32)
        x, y, w, h = self.by["head"]["bbox"]
        head[y : y + h, x : x + w] = self.pictures["head.png"]
        # just under the seam, where the dropped jaw uncovers it, the head holds the mouth's inside
        self.assertGreater(head[62, 64, 3], 200)
        self.assertLess(head[62, 64, :3].max(), 10)
        # further down the jaw always covers it, so the head and the background leave it out
        self.assertLess(head[72, 64, 3], 10)
        self.assertLess(self.result.background[72, 64].max(), 10)

    def test_a_float_takes_its_glow_and_leaves_the_ground(self):
        self.assertLess(self.result.background[8:16, 22:30].max(), 3)
        self.assertEqual(self.by["shard"]["float"], {"amp": [1, 2], "freq": 0.2, "rot_deg": 2})

    def test_surface_depths_ride_the_head(self):
        self.assertAlmostEqual(
            self.by["eye"]["depth"],
            fsm.surface_depth(tuple(self.by["eye"]["anchor"]), self.spec["head"]),
            places=3,
        )
        self.assertEqual(
            self.by["mouth_glow"]["depth"], fsm.surface_depth((64, 62), self.spec["head"])
        )

    def test_a_portrait_of_the_wrong_size_is_refused(self):
        with self.assertRaises(fsm.SpecError):
            split(self.img[:64], self.spec, stand_in_fill)


if __name__ == "__main__":
    unittest.main()
