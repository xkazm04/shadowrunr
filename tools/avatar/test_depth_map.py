# SPDX-License-Identifier: Apache-2.0
"""The depth-map prep on a small made-up rig: numpy and Pillow, no portrait, no model."""

from __future__ import annotations

import hashlib
import json
import tempfile
import unittest
from pathlib import Path

import numpy as np
from PIL import Image

import depth_map as dm

N = 256
HEAD = {"center": [128, 100], "radius": [48, 58], "pivot_y": 150, "chin_y": 155}


def _png(path: Path, rgba: np.ndarray) -> None:
    Image.fromarray(rgba.astype(np.uint8), "RGBA").save(path)


def make_rig(folder: Path, **overrides) -> None:
    """A skull-ish rig: a bright ellipse with two dark sockets, a jaw, a torso, an eye light and
    three shards. Every picture is drawn here; none comes from anybody's portrait."""
    ys, xs = np.mgrid[0:N, 0:N]
    u = ((xs - 128) / 48.0) ** 2 + ((ys - 100) / 58.0) ** 2
    head = np.zeros((N, N, 4))
    head[u <= 1] = [220, 210, 190, 255]
    for cx in (110, 146):  # the sockets
        head[((xs - cx) ** 2 + (ys - 92) ** 2) <= 9**2] = [8, 8, 10, 255]
    hx, hy, hw, hh = 80, 42, 96, 116
    _png(folder / "head.png", head[hy : hy + hh, hx : hx + hw])
    _png(folder / "jaw.png", np.full((20, 50, 4), [200, 190, 170, 255]))
    torso = np.zeros((126, N, 4))
    torso[:, 20:236] = [20, 20, 24, 255]
    _png(folder / "torso.png", torso)
    _png(folder / "eye_l.png", np.full((16, 16, 4), [40, 220, 200, 255]))
    _png(folder / "shard.png", np.full((24, 10, 4), [30, 30, 40, 255]))
    _png(folder / "background.png", np.full((N, N, 4), [5, 5, 5, 255]))
    spec = {
        "version": 1,
        "width": N,
        "height": N,
        "rig_type": "glow",
        "rig": {"head": HEAD, "aperture": {"seam_y": 132, "top": 120, "bottom": 250, "x0": 103, "x1": 153}},
        "layers": [
            {"name": "background", "kind": "static", "file": "background.png", "bbox": [0, 0, N, N]},
            {"name": "torso", "kind": "body", "file": "torso.png", "bbox": [0, 130, N, 126]},
            {"name": "head", "kind": "head", "file": "head.png", "bbox": [hx, hy, hw, hh]},
            {"name": "jaw", "kind": "aperture_lower", "file": "jaw.png", "bbox": [103, 135, 50, 20]},
            {"name": "shard_c", "kind": "float", "file": "shard.png", "bbox": [123, 4, 10, 24], "follow": 1.0},
            {"name": "shard_l", "kind": "float", "file": "shard.png", "bbox": [60, 30, 10, 24], "follow": 0.6},
            {"name": "shard_r", "kind": "float", "file": "shard.png", "bbox": [186, 30, 10, 24], "follow": 0.6},
            {"name": "eye_l", "kind": "eye", "file": "eye_l.png", "bbox": [102, 84, 16, 16], "mode": "light"},
        ],
    }
    spec.update(overrides)
    (folder / "layers.json").write_text(json.dumps(spec), encoding="utf-8")


class DepthMap(unittest.TestCase):
    def setUp(self):
        self._tmp = tempfile.TemporaryDirectory()
        self.dir = Path(self._tmp.name)
        make_rig(self.dir)

    def tearDown(self):
        self._tmp.cleanup()

    def depth(self) -> tuple[np.ndarray, dict]:
        meta = json.loads((self.dir / "depth.json").read_text(encoding="utf-8"))
        with Image.open(self.dir / "depth.png") as im:
            levels = np.asarray(im).astype(np.uint16)
        return dm.decode(levels, meta["near"], meta["far"]), meta

    def test_writes_a_16_bit_grayscale_map_at_the_rigs_size(self):
        dm.make(self.dir)
        with Image.open(self.dir / "depth.png") as im:
            self.assertEqual(im.mode, "I;16")
            self.assertEqual(im.size, (N, N))
        raw = (self.dir / "depth.png").read_bytes()
        self.assertEqual((raw[24], raw[25]), (16, 0))  # bit depth 16, colour type 0 (grayscale)

    def test_same_inputs_same_bytes(self):
        dm.make(self.dir)
        first = ((self.dir / "depth.png").read_bytes(), (self.dir / "depth.json").read_bytes())
        dm.make(self.dir)
        second = ((self.dir / "depth.png").read_bytes(), (self.dir / "depth.json").read_bytes())
        self.assertEqual(first, second)

    def test_the_json_names_its_inputs_range_and_method(self):
        meta = dm.make(self.dir)
        self.assertEqual(meta["method"], "analytic-v1")
        self.assertGreater(meta["near"], meta["far"])
        self.assertAlmostEqual(meta["scale"], (meta["near"] - meta["far"]) / 65535, places=8)
        self.assertEqual(
            meta["inputs"]["head.png"], hashlib.sha256((self.dir / "head.png").read_bytes()).hexdigest()
        )
        self.assertIn("layers.json", meta["inputs"])
        self.assertNotIn("background.png", meta["inputs"])  # the backplate is not the figure
        self.assertNotIn("null", (self.dir / "depth.json").read_text(encoding="utf-8"))

    def test_near_is_bright_and_the_head_stands_before_the_torso(self):
        dm.make(self.dir)
        z, _ = self.depth()
        with Image.open(self.dir / "depth.png") as im:
            levels = np.asarray(im)
        self.assertGreater(levels[110, 128], levels[220, 128])
        self.assertGreater(z[110, 128], z[220, 128] + 40)

    def test_the_sockets_are_carved(self):
        dm.make(self.dir)
        z, _ = self.depth()
        # the socket's centre against bone the same distance from the head's centre
        self.assertLess(z[92, 110], z[92, 128] - 15)
        self.assertLess(z[92, 110], z[108, 110] - 10)

    def test_the_jaw_sits_forward(self):
        dm.make(self.dir)
        with_jaw, _ = self.depth()
        spec = json.loads((self.dir / "layers.json").read_text(encoding="utf-8"))
        spec["layers"] = [l for l in spec["layers"] if l["name"] != "jaw"]
        (self.dir / "layers.json").write_text(json.dumps(spec), encoding="utf-8")
        dm.make(self.dir)
        without, _ = self.depth()
        self.assertGreater(with_jaw[145, 128], without[145, 128] + 5)

    def test_shards_stand_on_a_ring_by_their_place_and_follow(self):
        meta = dm.make(self.dir)
        planes = meta["planes"]
        self.assertEqual(set(planes), {"shard_c", "shard_l", "shard_r"})
        self.assertGreater(planes["shard_c"], planes["shard_l"])
        self.assertAlmostEqual(planes["shard_l"], planes["shard_r"], places=3)
        z, _ = self.depth()
        self.assertAlmostEqual(float(z[16, 128]), planes["shard_c"], delta=0.05)

    def test_the_figure_has_no_cliff_to_the_empty_canvas(self):
        dm.make(self.dir)
        z, _ = self.depth()
        # left of the torso's edge (x 20) the fill continues the torso's own depth
        self.assertAlmostEqual(float(z[200, 10]), float(z[200, 24]), delta=3.0)
        self.assertTrue(np.isfinite(z).all())

    def test_round_trips_within_one_level(self):
        rng = np.random.default_rng(3)
        d = rng.uniform(-60, 170, (8, 8)).astype(np.float32)
        levels, near, far = dm.encode(d)
        back = dm.decode(levels, near, far)
        self.assertLess(float(np.abs(back - d).max()), (near - far) / 65535 + 2e-3)

    def test_refuses_what_it_cannot_read(self):
        with self.assertRaises(ValueError):
            dm.make(self.dir, method="learned-v9")
        make_rig(self.dir, rig_type="mesh")
        with self.assertRaises(ValueError):
            dm.make(self.dir)
        self.assertEqual(dm.main([str(self.dir / "missing")]), 1)


class PushFill(unittest.TestCase):
    def test_takes_the_nearest_known_value(self):
        value = np.zeros((32, 32), np.float32)
        known = np.zeros((32, 32), np.float32)
        value[:, :8] = 10.0
        known[:, :8] = 1.0
        value[:, 24:] = 50.0
        known[:, 24:] = 1.0
        out = dm.push_fill(value, known)
        self.assertAlmostEqual(float(out[16, 2]), 10.0, places=4)
        self.assertLess(float(out[16, 10]), 20.0)
        self.assertGreater(float(out[16, 22]), 40.0)


if __name__ == "__main__":
    unittest.main()
