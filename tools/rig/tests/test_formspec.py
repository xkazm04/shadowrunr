# SPDX-License-Identifier: Apache-2.0
"""The region spec and layers.json checks: standard library only, no numpy, no pictures."""

from __future__ import annotations

import copy
import json
import tempfile
import unittest
from pathlib import Path

from shadowrunr_rig import formspec as fsm

MALAKOR = Path(__file__).resolve().parents[3] / "characters" / "malakor" / "rig" / "m8.spec.json"


def minimal() -> dict:
    return {
        "rig_type": "glow",
        "width": 64,
        "height": 64,
        "head": {"center": [32, 28], "radius": [20, 24], "chin_y": 50},
        "elements": [
            {
                "name": "head",
                "kind": "head",
                "mode": "cut",
                "region": {"poly": [[10, 4], [54, 4], [54, 52], [10, 52]]},
            },
        ],
    }


class RegionSpec(unittest.TestCase):
    def test_malakors_spec_checks(self):
        spec = fsm.load_spec(MALAKOR)
        names = [e["name"] for e in spec["elements"]]
        self.assertEqual(len(names), len(set(names)))
        kinds = {e["kind"] for e in spec["elements"]}
        self.assertTrue(
            {"head", "body", "aperture_lower", "aperture_glow", "eye", "pulse", "float"} <= kinds
        )
        self.assertEqual(sum(1 for e in spec["elements"] if e["kind"] == "float"), 11)
        # the jaw hinges alone: no upper lip, so the upper teeth stay with the skull
        self.assertNotIn("aperture_upper", kinds)

    def test_a_minimal_spec_checks(self):
        self.assertIs(fsm.check_spec(minimal())["rig_type"], "glow")

    def test_refuses_what_it_cannot_split(self):
        cases = {
            "not a glow rig": lambda s: s.update(rig_type="mesh"),
            "no head element": lambda s: s["elements"][0].update(kind="body"),
            "unknown kind": lambda s: s["elements"][0].update(kind="tail"),
            "static is the background's": lambda s: s["elements"][0].update(kind="static"),
            "unknown mode": lambda s: s["elements"][0].update(mode="melt"),
            "two region shapes": lambda s: s["elements"][0]["region"].update(ring=[1, 2, 3]),
            "a two-point polygon": lambda s: s["elements"][0]["region"].update(
                poly=[[0, 0], [1, 1]]
            ),
            "a reserved name": lambda s: s["elements"][0].update(name="background"),
            "a key fill on a solid": lambda s: s["elements"][0].update(
                fill="key", key={"base": [0, 0, 0]}
            ),
            "colours out of range": lambda s: s.update(colors={"mouth": [0, 2, 0]}),
            "an unknown light": lambda s: s.update(colors={"halo": [0, 1, 0]}),
            "a ground without a fade": lambda s: s.update(
                ground={"y": 60, "half_width": 20, "curve": 4}
            ),
            "a bad depth": lambda s: s["elements"][0].update(depth="deep"),
        }
        for why, mutate in cases.items():
            with self.subTest(why):
                s = minimal()
                mutate(s)
                with self.assertRaises(fsm.SpecError):
                    fsm.check_spec(s)

    def test_load_names_the_file_it_could_not_read(self):
        with tempfile.TemporaryDirectory() as d:
            p = Path(d) / "broken.json"
            p.write_text("{", encoding="utf-8")
            with self.assertRaisesRegex(fsm.SpecError, "broken.json"):
                fsm.load_spec(p)


class LayersJson(unittest.TestCase):
    head = {"center": [100, 100], "radius": [50, 50], "chin_y": 150}

    def test_surface_depth_follows_the_head_sphere(self):
        self.assertEqual(fsm.surface_depth((100, 100), self.head), 0.6)
        self.assertEqual(fsm.surface_depth((150, 100), self.head), 0.0)
        self.assertEqual(fsm.surface_depth((400, 100), self.head), 0.0)
        self.assertAlmostEqual(fsm.surface_depth((130, 140), self.head), 0.0, places=3)
        self.assertAlmostEqual(
            fsm.surface_depth((125, 100), self.head), round(0.6 * (0.75**0.5), 3)
        )

    def test_an_entry_omits_what_it_does_not_have(self):
        el = {
            "name": "jaw",
            "kind": "aperture_lower",
            "mode": "cut",
            "occl": 1.0,
            "region": {},
            "fill": "keep",
        }
        e = fsm.layer_entry(el, 3, (90, 110, 20, 40), self.head, "jaw.png")
        self.assertEqual(e["bbox"], [90, 110, 20, 40])
        self.assertEqual(e["anchor"], [100.0, 130.0])
        self.assertNotIn("depth", e)
        self.assertNotIn("region", e)
        self.assertNotIn("fill", e)
        self.assertEqual(e["occl"], 1.0)
        self.assertNotIn(None, e.values())

    def test_a_surface_depth_is_taken_at_its_anchor_or_where_it_says(self):
        el = {"name": "eye", "kind": "eye", "mode": "light", "depth": "surface"}
        self.assertEqual(
            fsm.layer_entry(el, 1, (90, 90, 20, 20), self.head, "eye.png")["depth"], 0.6
        )
        glow = {
            "name": "g",
            "kind": "aperture_glow",
            "mode": "none",
            "depth": "surface",
            "depth_at": [125, 100],
        }
        e = fsm.layer_entry(glow, 2, None, self.head, None)
        self.assertNotIn("bbox", e)
        self.assertNotIn("file", e)
        self.assertAlmostEqual(e["depth"], 0.52, places=2)
        with self.assertRaises(fsm.SpecError):
            fsm.layer_entry({**glow, "depth_at": None}, 2, None, self.head, None)

    def test_the_rig_block_carries_head_aperture_ground_and_lights(self):
        spec = minimal()
        spec["aperture"] = {
            "seam_y": 40,
            "top": 30,
            "bottom": 60,
            "x0": 20,
            "x1": 44,
            "open_frac": 0.1,
            "note": "x",
        }
        spec["ground"] = {"y": 60, "half_width": 20, "fade": 10, "curve": 4}
        spec["colors"] = {"mouth": [0.2, 0.85, 0.78]}
        rig = fsm.rig_section(spec)
        self.assertEqual(rig["head"], {"center": [32, 28], "radius": [20, 24], "chin_y": 50})
        self.assertEqual(
            rig["aperture"],
            {"seam_y": 40, "open_frac": 0.1, "top": 30, "bottom": 60, "x0": 20, "x1": 44},
        )
        self.assertEqual(rig["colors"], {"mouth": [0.2, 0.85, 0.78]})
        self.assertNotIn("colors", fsm.rig_section(minimal()))

    def test_check_formspec_refuses_what_the_stage_refuses(self):
        good = fsm.formspec(
            minimal(),
            "t",
            [{"name": "head", "z": 1, "kind": "head", "file": "head.png", "bbox": [0, 0, 8, 8]}],
        )
        fsm.check_formspec(good)
        cases = {
            "a path for a file": lambda f: f["layers"][0].update(file="../head.png"),
            "not a png": lambda f: f["layers"][0].update(file="head.webp"),
            "a short bbox": lambda f: f["layers"][0].update(bbox=[0, 0, 8]),
            "an unknown kind": lambda f: f["layers"][0].update(kind="ground"),
            "a null field": lambda f: f["layers"][0].update(depth=None),
            "no layers": lambda f: f.update(layers=[]),
            "a half aperture": lambda f: f["rig"].update(aperture={"seam_y": 4}),
            "no chin": lambda f: f["rig"]["head"].pop("chin_y"),
        }
        for why, mutate in cases.items():
            with self.subTest(why):
                f = copy.deepcopy(good)
                mutate(f)
                with self.assertRaises(fsm.SpecError):
                    fsm.check_formspec(f)

    def test_writes_layers_json_only_when_it_checks(self):
        with tempfile.TemporaryDirectory() as d:
            fs = fsm.formspec(
                minimal(),
                "t",
                [
                    {
                        "name": "head",
                        "z": 1,
                        "kind": "head",
                        "file": "head.png",
                        "bbox": [0, 0, 8, 8],
                    }
                ],
            )
            p = fsm.write_layers_json(fs, Path(d))
            self.assertEqual(json.loads(p.read_text(encoding="utf-8"))["rig_type"], "glow")
            fs["layers"] = []
            with self.assertRaises(fsm.SpecError):
                fsm.write_layers_json(fs, Path(d))


if __name__ == "__main__":
    unittest.main()
