# SPDX-License-Identifier: Apache-2.0
"""Stage a candidate rig, judge it per element and at its extremes, promote it, revert it.

Every picture here is drawn by the test (the bust of tests/test_split.py) and split with the
stand-in inpainter; nothing comes from anybody's portrait and nothing touches ``data/``."""

from __future__ import annotations

import contextlib
import io
import json
import os
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
    from shadowrunr_rig import rigdir
    from shadowrunr_rig.__main__ import main
    from shadowrunr_rig.split import Split, assemble, debug_overlay, split

    from .test_rigdir import depth_by_hand
    from .test_split import H, W, stand_in_fill
    from .test_split import portrait as drawn_portrait
    from .test_split import spec as drawn_spec

try:
    from shadowrunr_rig import review
except ImportError:  # today: there is no review module (the cases below are red)
    review = None

#: how far the misplaced element's polygon is pushed off its feature
OFF = 4


def misplaced_spec() -> dict:
    """The drawn spec with the jaw's polygon 4 px to the right of the jaw."""
    spec = drawn_spec()
    jaw = next(el for el in spec["elements"] if el["name"] == "jaw")
    jaw["region"]["poly"] = [[x + OFF, y] for x, y in jaw["region"]["poly"]]
    return spec


def tree(folder: Path) -> dict[str, bytes]:
    """Every file under `folder` (relative path -> bytes)."""
    return {
        p.relative_to(folder).as_posix(): p.read_bytes()
        for p in sorted(Path(folder).rglob("*"))
        if p.is_file()
    }


def run_main(argv: list[str]) -> int | str:
    """main's exit code, or what argparse refused (an unknown mode is a wrong answer, not a
    crash of the test)."""
    try:
        return main(argv)
    except SystemExit as e:
        return f"argparse exit {e.code}"


def need_review(test):
    test.assertIsNotNone(review, "shadowrunr_rig.review does not exist")


@unittest.skipIf(np is None, "numpy or Pillow is not installed")
class Runs(unittest.TestCase):
    """The tool end to end on a drawn portrait in a temporary folder."""

    def setUp(self):
        self._tmp = tempfile.TemporaryDirectory()
        root = Path(self._tmp.name)
        self.live = root / "rig"
        self.next = root / "rig.next"
        self.prev = root / "rig.prev"
        self.portrait = root / "portrait.png"
        self.good = root / "good.json"
        self.bad = root / "bad.json"
        Image.fromarray(np.rint(drawn_portrait()).astype(np.uint8)).save(self.portrait)
        self.good.write_text(json.dumps(drawn_spec()), encoding="utf-8")
        self.bad.write_text(json.dumps(misplaced_spec()), encoding="utf-8")

    def tearDown(self):
        self._tmp.cleanup()

    def split_into(self, spec: Path, *extra: str, code: int = 0) -> str:
        class StandIn:
            def __init__(self, _path, _device="cuda"):
                self.fill = stand_in_fill

        argv = ["--portrait", str(self.portrait), "--spec", str(spec), "--out", str(self.live)]
        argv += ["--lama", "stand-in.pt", "--device", "cpu", *extra]
        buf = io.StringIO()
        with mock.patch("shadowrunr_rig.lama.Lama", StandIn), contextlib.redirect_stdout(buf):
            self.assertEqual(run_main(argv), code, buf.getvalue())
        return buf.getvalue()

    def promote(self, *extra: str, code: int = 0) -> str:
        buf = io.StringIO()
        with contextlib.redirect_stdout(buf), contextlib.redirect_stderr(buf):
            got = run_main(["promote", "--out", str(self.live), *extra])
        self.assertEqual(got, code, buf.getvalue())
        return buf.getvalue()

    # case 1
    def test_a_candidate_run_leaves_the_live_rig_alone(self):
        self.split_into(self.good)
        Image.fromarray(np.full((4, 4, 3), 9, np.uint8)).save(self.live / "notes.png")
        depth_by_hand(self.live)
        before = tree(self.live)
        self.split_into(self.bad, "--candidate")
        self.assertEqual(tree(self.live), before)
        staged = rigdir.load(self.next)  # a whole rig, checked as the stage checks it
        for file in rigdir.owned_files(staged.fs):
            self.assertTrue((self.next / file).is_file(), file)
        self.assertTrue((self.next / "report.json").is_file())
        self.assertTrue((self.next / "extremes.png").is_file())

    # case 4
    def test_promote_refuses_an_element_that_got_worse_unless_accepted(self):
        self.split_into(self.good)
        self.assertTrue((self.live / "report.json").is_file())
        self.split_into(self.bad, "--candidate")
        before = tree(self.live)
        printed = self.promote(code=1)
        self.assertEqual(tree(self.live), before)
        line = next(ln for ln in printed.splitlines() if "worse" in ln)
        self.assertIn("jaw", line)
        self.assertIn("->", line)
        self.assertNotIn("head", [ln.split()[0] for ln in printed.splitlines() if "worse" in ln])
        staged = (self.next / "layers.json").read_bytes()
        self.promote("--accept")
        self.assertEqual((self.live / "layers.json").read_bytes(), staged)

    # case 5
    def test_promote_keeps_the_previous_rig_and_revert_brings_it_back_byte_for_byte(self):
        self.split_into(self.good)
        Image.fromarray(np.full((4, 4, 3), 9, np.uint8)).save(self.live / "notes.png")
        depth_by_hand(self.live)
        self.split_into(self.bad, "--candidate")
        before = tree(self.live)
        staged = tree(self.next)
        writes: list[str] = []
        real = os.replace

        def spy(src, dst):
            if Path(dst).parent == self.live:
                writes.append(Path(dst).name)
            return real(src, dst)

        with mock.patch.object(rigdir.os, "replace", spy):
            self.promote("--accept")
        owned = rigdir.owned_files(json.loads(staged["layers.json"]))
        rig_writes = [n for n in writes if n in owned]
        self.assertEqual(sorted(rig_writes), sorted(owned))  # every rig file, through rigdir
        self.assertEqual(rig_writes[-1], "layers.json")  # and layers.json last
        self.assertEqual((self.live / "layers.json").read_bytes(), staged["layers.json"])
        for name in rigdir.owned_files(rigdir.load(self.live).fs):
            self.assertEqual((self.live / name).read_bytes(), staged[name], name)
        kept = tree(self.prev)
        for name, data in before.items():
            if name != "notes.png":  # not the rig's: promote never touched it
                self.assertEqual(kept.get(name), data, name)
        self.promote("--revert")
        self.assertEqual(tree(self.live), before)

    # pins: promotion never loses the previous rig
    def test_the_previous_rig_survives_a_second_promote_and_a_changed_live_rig(self):
        self.split_into(self.good)
        good = tree(self.live)
        self.split_into(self.bad, "--candidate")
        self.promote("--accept")
        kept = tree(self.prev)
        self.promote(code=1)  # nothing staged: a second promote does not overwrite rig.prev
        self.assertEqual(tree(self.prev), kept)
        (self.live / "layers.json").write_bytes((self.live / "layers.json").read_bytes() + b" ")
        self.promote("--revert", code=1)  # live changed since the promote: revert would lose it
        self.assertEqual(tree(self.prev), kept)
        (self.live / "layers.json").write_bytes((self.live / "layers.json").read_bytes()[:-1])
        self.promote("--revert")
        self.assertEqual(tree(self.live), good)


@unittest.skipIf(np is None, "numpy or Pillow is not installed")
class Judging(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.img = drawn_portrait()

    def judged(self, spec: dict):
        result = split(self.img, spec, stand_in_fill)
        fs, pictures = assemble(spec, result, "test")
        return result, fs, pictures

    # case 2 (revised)
    def test_a_misplaced_element_ranks_worst_and_the_others_stay_clean(self):
        need_review(self)
        result, fs, pictures = self.judged(misplaced_spec())
        rep = review.element_report(result, fs, pictures, self.img)
        rows = rep["elements"]
        for row in rows:
            for key in ("name", "bbox", "coverage_px", "filled_px", "error"):
                self.assertIn(key, row)
        worst = max(rows, key=lambda r: r["error"]["p99"])
        self.assertEqual(worst["name"], "jaw")
        self.assertEqual(rows[0]["name"], "jaw")  # the report lists the worst first
        for row in rows:
            if row["name"] != "jaw" and row["kind"] != "static":
                self.assertLess(row["error"]["p99"], 8, row["name"])
        jaw = next(r for r in rows if r["name"] == "jaw")
        self.assertGreater(jaw["filled_px"], 0)
        # every pixel of the frame is owned by exactly one row
        self.assertEqual(sum(r["owned_px"] for r in rows), W * H)
        # and in the well-placed split no non-static element reaches 8
        result, fs, pictures = self.judged(drawn_spec())
        for row in review.element_report(result, fs, pictures, self.img)["elements"]:
            if row["kind"] != "static":
                self.assertLess(row["error"]["p99"], 8, row["name"])

    # case 3
    def test_the_extremes_sheet_drops_the_jaw_and_lifts_the_floats(self):
        need_review(self)
        spec = drawn_spec()
        _, fs, pictures = self.judged(spec)
        sheet = review.extremes(fs, pictures, spec)
        self.assertEqual(sheet.shape, (H, 3 * W, 3))
        rest, jaw_open, floats = sheet[:, :W], sheet[:, W : 2 * W], sheet[:, 2 * W :]
        expect = np.clip(np.rint(rigdir.compose_rest(fs, pictures)), 0, 255).astype(np.uint8)
        np.testing.assert_array_equal(rest, expect)

        def inside(box, dx=0, dy=0):
            x, y, w, h = box
            m = np.zeros((H, W), bool)
            m[max(0, y + dy) : max(0, y + dy + h), max(0, x + dx) : max(0, x + dx + w)] = True
            return m

        by = {layer["name"]: layer for layer in fs["layers"]}
        jaw_box = by["jaw"]["bbox"]
        moved = (jaw_open != rest).any(axis=2)
        self.assertTrue(moved.any())
        self.assertFalse((moved & ~(inside(jaw_box) | inside(jaw_box, dy=6))).any())
        dx, dy = (int(round(v)) for v in by["shard"]["float"]["amp"])
        shard_box = by["shard"]["bbox"]
        lifted = (floats != rest).any(axis=2)
        self.assertTrue(lifted.any())
        self.assertFalse((lifted & ~(inside(shard_box) | inside(shard_box, dx, dy))).any())

    # case 6
    def test_region_colours_follow_the_name_and_a_legend_names_them(self):
        need_review(self)
        result, _, _ = self.judged(drawn_spec())
        names = list(result.coverage)
        flipped = Split(
            background=result.background,
            lifted=result.lifted,
            coverage=dict(reversed(list(result.coverage.items()))),
        )
        np.testing.assert_array_equal(debug_overlay(result), debug_overlay(flipped))
        alone = Split(result.background, [], {"jaw": result.coverage["jaw"]})
        jaw_only = result.coverage["jaw"] > 0.99
        for other, cov in result.coverage.items():
            if other != "jaw":
                jaw_only &= cov < 0.01
        self.assertTrue(jaw_only.any())
        np.testing.assert_array_equal(
            debug_overlay(alone)[jaw_only], debug_overlay(result)[jaw_only]
        )
        sheet = review.regions(result)
        self.assertEqual(sheet.shape[1], W)
        np.testing.assert_array_equal(sheet[:H], debug_overlay(result))
        legend = sheet[H:]
        self.assertEqual(legend.shape[0], len(names) * review.LEGEND_ROW)
        for i, name in enumerate(names):
            band = legend[i * review.LEGEND_ROW : (i + 1) * review.LEGEND_ROW]
            colour = np.asarray(review.element_colour(name), np.uint8)
            swatch = (band == colour).all(axis=2)
            self.assertTrue(swatch.any(), f"{name} has no swatch in its legend row")
            # and its name is written beside the swatch
            right = band[:, int(np.nonzero(swatch.any(axis=0))[0].max()) + 2 :]
            self.assertTrue((right > 0).any(), f"{name} is not written in the legend")
        # same name, same colour: from the name alone, never from a run's random state
        self.assertEqual(review.element_colour("jaw"), review.element_colour("ja" + "w"))
        self.assertNotEqual(review.element_colour("jaw"), review.element_colour("head"))


if __name__ == "__main__":
    unittest.main()
