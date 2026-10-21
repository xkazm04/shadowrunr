# SPDX-License-Identifier: Apache-2.0
"""The rig folder's one owner: load, compose, publish and depth freshness, on drawn rigs only.

Every picture here is drawn by the test; none comes from anybody's portrait, and nothing is read
from or written to ``data/``."""

from __future__ import annotations

import contextlib
import hashlib
import io
import json
import tempfile
import unittest
from pathlib import Path
from unittest import mock

try:
    import numpy as np
    from PIL import Image
except ImportError:  # pragma: no cover - the interpreter has no numpy or Pillow
    np = None

if np is not None:
    from shadowrunr_rig import formspec as fsm
    from shadowrunr_rig import rigdir, split
    from shadowrunr_rig.__main__ import main

    from .test_split import portrait as drawn_portrait
    from .test_split import spec as drawn_spec
    from .test_split import stand_in_fill

W = H = 32
STALE = "depth map is stale: run tools/avatar/depth_map.py"


def small_rig(tint: int = 200) -> tuple[dict, dict]:
    """A FormSpec and its pictures: a dark backplate, a skull cut and an eye light."""
    bg = np.zeros((H, W, 4), np.uint8)
    bg[..., :3] = 6
    bg[..., 3] = 255
    head = np.zeros((16, 12, 4), np.uint8)
    head[2:14, 1:11] = [tint, tint - 10, tint - 30, 255]
    eye = np.zeros((6, 6, 4), np.uint8)
    eye[1:5, 1:5] = [20, 180, 160, 255]
    fs = {
        "version": 1,
        "portrait_id": "drawn",
        "source": "glow",
        "rig_type": "glow",
        "width": W,
        "height": H,
        "layers": [
            {
                "name": "background",
                "z": 1,
                "kind": "static",
                "file": "background.png",
                "bbox": [0, 0, W, H],
            },
            {
                "name": "head",
                "z": 2,
                "kind": "head",
                "file": "head.png",
                "bbox": [10, 6, 12, 16],
                "mode": "cut",
            },
            {
                "name": "eye",
                "z": 3,
                "kind": "eye",
                "file": "eye.png",
                "bbox": [12, 10, 6, 6],
                "mode": "light",
            },
        ],
        "rig": {"head": {"center": [16, 14], "radius": [6, 8], "chin_y": 22}},
    }
    return fs, {"background.png": bg, "head.png": head, "eye.png": eye}


def write_by_hand(folder: Path, fs: dict, pictures: dict) -> None:
    """The rig as the old splitter left it: the PNGs, then layers.json."""
    folder.mkdir(parents=True, exist_ok=True)
    for name, pic in pictures.items():
        Image.fromarray(pic).save(folder / name, compress_level=6)
    (folder / "layers.json").write_text(json.dumps(fs, indent=1) + "\n", encoding="utf-8")


def depth_by_hand(folder: Path) -> None:
    """What tools/avatar/depth_map.py leaves beside the rig: depth.png and depth.json, whose
    `inputs` are the sha256 of layers.json and of every figure layer's PNG."""
    fs = json.loads((folder / "layers.json").read_text(encoding="utf-8"))
    names = ["layers.json"] + [
        layer["file"] for layer in fs["layers"] if layer.get("file") and layer["kind"] != "static"
    ]
    inputs = {n: hashlib.sha256((folder / n).read_bytes()).hexdigest() for n in sorted(names)}
    Image.fromarray(np.zeros((H, W), np.uint16)).save(folder / "depth.png")
    meta = {"version": 1, "width": W, "height": H, "near": 1.0, "far": 0.0, "inputs": inputs}
    (folder / "depth.json").write_text(json.dumps(meta, sort_keys=True), encoding="utf-8")


@unittest.skipIf(np is None, "numpy or Pillow is not installed")
class RigDir(unittest.TestCase):
    def setUp(self):
        self._tmp = tempfile.TemporaryDirectory()
        self.dir = Path(self._tmp.name) / "rig"
        fs, pictures = small_rig()
        rigdir.publish(self.dir, fs, pictures)
        depth_by_hand(self.dir)

    def tearDown(self):
        self._tmp.cleanup()

    # case 1: a new rig takes the depth pair with it, together, and says so
    def test_a_changed_rig_removes_depth_png_and_json_together_and_says_so(self):
        fs, pictures = small_rig(tint=150)
        report = rigdir.publish(self.dir, fs, pictures)
        self.assertFalse((self.dir / "depth.png").exists())
        self.assertFalse((self.dir / "depth.json").exists())
        self.assertEqual(report.depth, "stale")
        self.assertIn(f"{STALE} {self.dir}", "\n".join(report.notes))

    def test_a_byte_identical_rig_keeps_its_depth_pair(self):
        fs, pictures = small_rig()
        report = rigdir.publish(self.dir, fs, pictures)
        self.assertTrue((self.dir / "depth.png").exists())
        self.assertTrue((self.dir / "depth.json").exists())
        self.assertEqual(report.depth, "fresh")
        self.assertEqual(rigdir.depth_status(self.dir).state, "fresh")

    def test_an_orphan_depth_png_goes_with_the_rig_that_lost_its_json(self):
        (self.dir / "depth.json").unlink()
        fs, pictures = small_rig(tint=150)
        rigdir.publish(self.dir, fs, pictures)
        self.assertFalse((self.dir / "depth.png").exists())

    # case 2: only what the old manifest named is cleared
    def test_publish_clears_only_the_files_the_old_manifest_named(self):
        Image.fromarray(np.full((4, 4, 3), 9, np.uint8)).save(self.dir / "notes.png")
        before = (self.dir / "notes.png").read_bytes()
        fs, pictures = small_rig()
        fs["layers"] = [layer for layer in fs["layers"] if layer["name"] != "eye"]
        del pictures["eye.png"]
        rigdir.publish(self.dir, fs, pictures)
        self.assertEqual((self.dir / "notes.png").read_bytes(), before)
        self.assertFalse((self.dir / "eye.png").exists())  # the old manifest named it
        self.assertTrue((self.dir / "head.png").exists())
        self.assertEqual(rigdir.owned_files(fs), {"layers.json", "background.png", "head.png"})

    def test_publish_refuses_a_rig_it_cannot_load_back_and_leaves_the_folder(self):
        before = {p.name: p.read_bytes() for p in self.dir.iterdir()}
        fs, pictures = small_rig(tint=150)
        pictures["head.png"] = pictures["head.png"][:10]  # no longer its bbox
        with self.assertRaises(fsm.SpecError):
            rigdir.publish(self.dir, fs, pictures)
        self.assertEqual({p.name: p.read_bytes() for p in self.dir.iterdir()}, before)

    # case 3: freshness against depth.json's recorded inputs
    def test_depth_status_is_fresh_then_names_the_rewritten_layer(self):
        self.assertEqual(rigdir.depth_status(self.dir).state, "fresh")
        head = np.array(Image.open(self.dir / "head.png"))
        head[5, 5] = [1, 2, 3, 255]
        Image.fromarray(head).save(self.dir / "head.png")
        status = rigdir.depth_status(self.dir)
        self.assertEqual(status.state, "stale")
        self.assertEqual(status.changed, ["head.png"])

    def test_depth_status_without_depth_json_is_missing(self):
        (self.dir / "depth.json").unlink()
        self.assertEqual(rigdir.depth_status(self.dir).state, "missing")

    # load: the package's checks, and the picture is its bbox
    def test_load_refuses_an_escaping_file_and_a_picture_off_its_bbox(self):
        fs = json.loads((self.dir / "layers.json").read_text(encoding="utf-8"))
        fs["layers"][2]["file"] = "../escape.png"
        Image.fromarray(np.zeros((6, 6, 4), np.uint8)).save(self.dir.parent / "escape.png")
        (self.dir / "layers.json").write_text(json.dumps(fs), encoding="utf-8")
        with self.assertRaises(fsm.SpecError):
            rigdir.load(self.dir)
        fs["layers"][2]["file"] = "eye.png"
        fs["layers"][2]["bbox"] = [12, 10, 7, 6]
        (self.dir / "layers.json").write_text(json.dumps(fs), encoding="utf-8")
        with self.assertRaisesRegex(fsm.SpecError, "eye"):
            rigdir.load(self.dir)

    def test_load_gives_back_what_publish_wrote(self):
        fs, pictures = small_rig()
        loaded = rigdir.load(self.dir)
        self.assertEqual(loaded.fs, fs)
        for name, pic in pictures.items():
            np.testing.assert_array_equal(loaded.pictures[name], pic)
        self.assertEqual(loaded.files["layers.json"], (self.dir / "layers.json").read_bytes())


@unittest.skipIf(np is None, "numpy or Pillow is not installed")
class OneCompositor(unittest.TestCase):
    def test_split_reexports_the_one_compose_rest(self):
        self.assertIs(split.compose_rest, rigdir.compose_rest)

    def test_a_light_adds_at_gain_and_a_cut_occludes_by_alpha_times_occl(self):
        fs, pictures = small_rig()
        fs["layers"][1]["gain"] = 0.5
        fs["layers"][1]["occl"] = 0.5
        pictures["head.png"][..., 3] = np.where(pictures["head.png"][..., 3] > 0, 128, 0)
        pictures["eye.png"][..., 3] = np.where(pictures["eye.png"][..., 3] > 0, 128, 0)
        rest = rigdir.compose_rest(fs, pictures)
        a = 128 / 255
        head_px = pictures["head.png"][8, 8, :3].astype(np.float32)  # canvas (14, 18): head only
        np.testing.assert_allclose(rest[14, 18], 6 * (1 - a * 0.5) + head_px * 0.5 * a, atol=1e-3)
        eye_px = pictures["eye.png"][2, 2, :3].astype(np.float32)  # canvas (12, 14): eye over head
        under = 6 * (1 - a * 0.5) + pictures["head.png"][6, 4, :3].astype(np.float32) * 0.5 * a
        np.testing.assert_allclose(rest[12, 14], np.clip(under + eye_px, 0, 255), atol=1e-3)


@unittest.skipIf(np is None, "numpy or Pillow is not installed")
class MainPublishes(unittest.TestCase):
    """`python -m shadowrunr_rig` writes through rigdir.publish (case 1 and 2, end to end)."""

    def setUp(self):
        self._tmp = tempfile.TemporaryDirectory()
        root = Path(self._tmp.name)
        self.out = root / "rig"
        self.portrait = root / "portrait.png"
        self.spec = root / "spec.json"
        Image.fromarray(np.rint(drawn_portrait()).astype(np.uint8)).save(self.portrait)
        self.spec.write_text(json.dumps(drawn_spec()), encoding="utf-8")

    def tearDown(self):
        self._tmp.cleanup()

    def run_main(self) -> str:
        class StandIn:
            def __init__(self, _path, _device="cuda"):
                self.fill = stand_in_fill

        argv = ["--portrait", str(self.portrait), "--spec", str(self.spec), "--out", str(self.out)]
        argv += ["--lama", "stand-in.pt", "--device", "cpu"]
        buf = io.StringIO()
        with mock.patch("shadowrunr_rig.lama.Lama", StandIn), contextlib.redirect_stdout(buf):
            self.assertEqual(main(argv), 0)
        return buf.getvalue()

    def test_a_resplit_keeps_unrelated_files_and_invalidates_the_depth_pair(self):
        self.run_main()
        loaded = rigdir.load(self.out)
        self.assertIn("jaw.png", loaded.pictures)
        Image.fromarray(np.full((4, 4, 3), 9, np.uint8)).save(self.out / "notes.png")
        # a depth map made for some other rig of this size
        depth_by_hand(self.out)
        meta = json.loads((self.out / "depth.json").read_text(encoding="utf-8"))
        meta["inputs"]["jaw.png"] = "0" * 64
        (self.out / "depth.json").write_text(json.dumps(meta), encoding="utf-8")
        printed = self.run_main()
        self.assertTrue((self.out / "notes.png").exists())
        self.assertFalse((self.out / "depth.png").exists())
        self.assertFalse((self.out / "depth.json").exists())
        self.assertIn(STALE, printed)


if __name__ == "__main__":
    unittest.main()
