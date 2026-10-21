# SPDX-License-Identifier: Apache-2.0
"""The `learned-v1` depth map on the made-up rig, with a fake model step: no portrait, no model,
no onnxruntime."""

from __future__ import annotations

import json
import tempfile
import unittest
from pathlib import Path

import numpy as np
from PIL import Image

import depth_map as dm
import learned_depth as ld
from test_depth_map import HEAD, N, make_rig

FAKE_SHA = "f" * 64


def fake_model(gain: float = 0.02, offset: float = 3.0, lean: float = 0.0):
    """A stand-in for the model: relative inverse depth from the made-up rig's geometry (the
    head's ellipsoid over a flat torso), at any gain and offset, leaning nearer toward the
    bottom by `lean` (as the model reads a portrait), at the model's input size."""

    def infer(_model: Path, pixels: np.ndarray) -> np.ndarray:
        side = pixels.shape[-1]
        k = side / N
        ys, xs = np.mgrid[0:side, 0:side].astype(np.float32)
        (cx, cy), (rx, ry) = HEAD["center"], HEAD["radius"]
        u = ((xs / k - cx) / rx) ** 2 + ((ys / k - cy) / ry) ** 2
        z = dm.HEAD_FLOOR + dm.HEAD_DEPTH_FRAC * rx * np.sqrt(np.clip(1.0 - u, 0.0, 1.0))
        z = np.where(u <= 1.0, z, 0.0) + lean * (ys / k - cy) / ry
        return (gain * z + offset).astype(np.float32)

    return infer


class LearnedDepth(unittest.TestCase):
    def setUp(self):
        self._tmp = tempfile.TemporaryDirectory()
        self.dir = Path(self._tmp.name)
        make_rig(self.dir)

    def tearDown(self):
        self._tmp.cleanup()

    def step(self, infer=None) -> dict:
        """What `depth_map.py --method learned-v1` asks for: the input, then the model step."""
        if not (self.dir / dm.LEARNED_OUT).exists():
            with self.assertRaises(dm.LearnedPending):
                dm.make(self.dir, "learned-v1")
        ld.run(self.dir, Path("model.onnx"), infer_fn=infer or fake_model(), check_fn=lambda _p: FAKE_SHA)
        return dm.make(self.dir, "learned-v1")

    def depth(self) -> np.ndarray:
        meta = json.loads((self.dir / "depth.json").read_text(encoding="utf-8"))
        with Image.open(self.dir / "depth.png") as im:
            return dm.decode(np.asarray(im).astype(np.uint16), meta["near"], meta["far"])

    def test_waits_for_the_model_step_and_writes_its_fixed_input(self):
        with self.assertRaises(dm.LearnedPending):
            dm.make(self.dir, "learned-v1")
        self.assertFalse((self.dir / "depth.png").exists())
        pixels = np.load(self.dir / dm.LEARNED_IN)
        self.assertEqual((pixels.dtype, pixels.shape), (np.float32, (1, 3, dm.MODEL_SIDE, dm.MODEL_SIDE)))
        # the backplate is in the picture the model sees: (5/255 - mean) / std in its corner
        self.assertAlmostEqual(float(pixels[0, 0, 0, 0]), (5 / 255 - 0.485) / 0.229, places=3)
        self.assertEqual(dm.main([str(self.dir), "--method", "learned-v1"]), 2)

    def test_the_json_names_the_method_the_model_and_the_calibration(self):
        meta = self.step()
        self.assertEqual(meta["method"], "learned-v1")
        self.assertEqual(meta["model"]["id"], ld.MODEL_ID)
        self.assertEqual(meta["model"]["revision"], ld.MODEL_REVISION)
        self.assertEqual(meta["model"]["sha256"], FAKE_SHA)
        self.assertEqual(set(meta["calibration"]), {"gain", "offset", "tilt", "clipped"})
        self.assertIn("background.png", meta["inputs"])  # the model saw the backplate
        self.assertNotIn("null", (self.dir / "depth.json").read_text(encoding="utf-8"))

    def test_same_inputs_same_bytes(self):
        self.step()
        first = ((self.dir / "depth.png").read_bytes(), (self.dir / "depth.json").read_bytes())
        dm.make(self.dir, "learned-v1")
        second = ((self.dir / "depth.png").read_bytes(), (self.dir / "depth.json").read_bytes())
        self.assertEqual(first, second)

    def test_lands_on_the_heads_ellipsoid_whatever_the_models_scale(self):
        self.step(fake_model(gain=0.02, offset=3.0))
        a = self.depth()
        self.step(fake_model(gain=7.0, offset=-11.0))
        b = self.depth()
        cx, cy = HEAD["center"]
        # below the sockets, on the ellipsoid
        on_ellipsoid = dm.HEAD_FLOOR + dm.HEAD_DEPTH_FRAC * HEAD["radius"][0] * np.sqrt(1 - (16 / 58) ** 2)
        for z in (a, b):
            self.assertAlmostEqual(float(z[cy + 16, cx]), on_ellipsoid, delta=3.0)
            self.assertGreater(float(z[cy + 16, cx]), float(z[220, cx]) + 40)  # the head before the torso
        self.assertLess(float(np.abs(a - b)[40:160, 80:176].max()), 1.0)

    def test_the_models_lean_is_taken_off_the_head(self):
        self.step(fake_model(lean=0.0))
        upright = self.depth()
        self.step(fake_model(lean=30.0))
        leaning = self.depth()
        meta = json.loads((self.dir / "depth.json").read_text(encoding="utf-8"))
        self.assertGreater(meta["calibration"]["tilt"][1], 20.0)
        cx, cy = HEAD["center"]
        for dy in (-30, 30):
            self.assertAlmostEqual(float(leaning[cy + dy, cx]), float(upright[cy + dy, cx]), delta=3.0)

    def test_the_shards_keep_their_planes_and_the_torso_its_slab(self):
        meta = self.step()
        self.assertEqual(meta["planes"], dm.analytic_v1(dm.load_rig(self.dir)).planes)
        z = self.depth()
        self.assertAlmostEqual(float(z[16, 128]), meta["planes"]["shard_c"], delta=0.05)
        # the fake torso is flat, so it has no relief: the analytic slab
        slab = dm.torso_surface(dm.load_rig(self.dir))
        self.assertAlmostEqual(float(z[220, 128]), float(slab[220, 128]), delta=1.0)

    def test_a_changed_rig_waits_for_the_model_again(self):
        self.step()
        Image.fromarray(np.full((20, 50, 4), [90, 90, 90, 255], np.uint8), "RGBA").save(self.dir / "jaw.png")
        with self.assertRaisesRegex(dm.LearnedPending, "another input"):
            dm.make(self.dir, "learned-v1")
        # and an output that is not the one the record names is not used
        ld.run(self.dir, Path("model.onnx"), infer_fn=fake_model(), check_fn=lambda _p: FAKE_SHA)
        np.save(self.dir / dm.LEARNED_OUT, np.zeros((4, 4), np.float32))
        with self.assertRaisesRegex(dm.LearnedPending, "not the file"):
            dm.make(self.dir, "learned-v1")

    def test_refuses_a_model_that_cannot_calibrate(self):
        flat = lambda _m, p: np.ones(p.shape[-2:], np.float32)  # noqa: E731
        with self.assertRaises(dm.LearnedPending):
            dm.make(self.dir, "learned-v1")
        ld.run(self.dir, Path("model.onnx"), infer_fn=flat, check_fn=lambda _p: FAKE_SHA)
        with self.assertRaisesRegex(ValueError, "centre before its rim"):
            dm.make(self.dir, "learned-v1")


class ModelStep(unittest.TestCase):
    def test_refuses_a_model_file_that_is_not_the_pinned_one(self):
        with tempfile.TemporaryDirectory() as tmp:
            fake = Path(tmp) / "model.onnx"
            fake.write_bytes(b"not the model")
            with self.assertRaisesRegex(ValueError, "not the pinned"):
                ld.check_model(fake)
            with self.assertRaisesRegex(ValueError, "no model"):
                ld.check_model(Path(tmp) / "missing.onnx")

    def test_needs_its_input_first(self):
        with tempfile.TemporaryDirectory() as tmp:
            with self.assertRaisesRegex(ValueError, "no learned.in.npy"):
                ld.run(Path(tmp), Path("model.onnx"), infer_fn=fake_model(), check_fn=lambda _p: FAKE_SHA)
            self.assertEqual(ld.main([tmp]), 1)


if __name__ == "__main__":
    unittest.main()
