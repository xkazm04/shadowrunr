# SPDX-License-Identifier: Apache-2.0
"""Blended voices over a synthetic voices file: no Kokoro file is read."""

from __future__ import annotations

import hashlib
import json
import struct
import tempfile
import unittest
from array import array
from pathlib import Path

from shadowrunr_voice.blend import (
    KOKORO_V1_SPEAKERS,
    ROW_FLOATS,
    STOCK_SID,
    Blend,
    BlendError,
    assign_slots,
    blend_rows,
    build,
    load_recipes,
    sid_of,
)

SPEAKERS = len(KOKORO_V1_SPEAKERS)
RECIPE = """
[voices.malakor-a]
am_onyx = 3
bm_lewis = 2

[voices.malakor-b]
am_onyx = 0.5
am_echo = 0.5
"""


def voices_bytes(speakers: int = SPEAKERS) -> bytes:
    """Speaker ``s`` is the constant ``s + 1``, except row floats that vary in ``am_onyx``."""
    body = bytearray()
    for s in range(speakers):
        body += struct.pack("<f", float(s + 1)) * ROW_FLOATS
    onyx = sid_of("am_onyx") * ROW_FLOATS * 4
    for i in range(0, ROW_FLOATS, 997):
        body[onyx + 4 * i : onyx + 4 * i + 4] = struct.pack("<f", i / 7.0)
    return bytes(body)


def speaker(raw: bytes, sid: int) -> array[float]:
    out = array("f")
    out.frombytes(raw[sid * ROW_FLOATS * 4 : (sid + 1) * ROW_FLOATS * 4])
    return out


class Names(unittest.TestCase):
    def test_the_table_has_the_models_53_speakers_and_af_heart_is_3(self) -> None:
        self.assertEqual(SPEAKERS, 53)
        self.assertEqual(sid_of("af_heart"), STOCK_SID)
        self.assertEqual(sid_of("am_onyx"), 17)
        self.assertEqual(len(set(KOKORO_V1_SPEAKERS)), SPEAKERS)

    def test_an_unknown_speaker_is_refused(self) -> None:
        with self.assertRaises(BlendError):
            sid_of("af_somebody_real")


class Recipes(unittest.TestCase):
    def test_weights_are_normalised_in_file_order(self) -> None:
        a, b = load_recipes({"voices": {"x": {"am_onyx": 3, "bm_lewis": 1}, "y": {"am_echo": 2}}})
        self.assertEqual(a.name, "x")
        self.assertEqual(a.weights, ((17, 0.75), (27, 0.25)))
        self.assertEqual(b.weights, ((12, 1.0),))

    def test_bad_recipes_are_refused(self) -> None:
        for table in (
            {},
            {"voices": {}},
            {"voices": {"x": {}}},
            {"voices": {"x": {"am_onyx": 0}}},
            {"voices": {"x": {"am_onyx": -1}}},
            {"voices": {"x": {"am_onyx": True}}},
            {"voices": {"x": {"nobody": 1}}},
            {"voices": {"x": 1}},
            {"voices": {"x": {"am_onyx": 1, "sid": "52"}}},
        ):
            with self.subTest(table=table), self.assertRaises(BlendError):
                load_recipes(table)


class Slots(unittest.TestCase):
    def test_blends_take_the_last_free_sids(self) -> None:
        blends = assign_slots([Blend("a", ((17, 0.5), (52, 0.5))), Blend("b", ((12, 1.0),))])
        self.assertEqual([b.sid for b in blends], [51, 50])  # 52 is an input

    def test_a_chosen_sid_is_kept_and_skipped_by_the_rest(self) -> None:
        blends = assign_slots([Blend("a", ((17, 1.0),)), Blend("b", ((12, 1.0),), sid=52)])
        self.assertEqual([b.sid for b in blends], [51, 52])

    def test_the_stock_voice_an_input_or_a_taken_slot_is_never_replaced(self) -> None:
        for blends in (
            [Blend("a", ((17, 1.0),), sid=STOCK_SID)],
            [Blend("a", ((17, 1.0),), sid=17)],
            [Blend("a", ((17, 1.0),), sid=40), Blend("b", ((12, 1.0),), sid=40)],
            [Blend("a", ((17, 1.0),), sid=SPEAKERS)],
        ):
            with self.subTest(blends=blends), self.assertRaises(BlendError):
                assign_slots(blends)

    def test_no_default_slot_is_ever_the_stock_voice(self) -> None:
        inputs = tuple((s, 1.0) for s in range(4, SPEAKERS))
        (only,) = assign_slots([Blend("a", inputs)])
        self.assertIn(only.sid, (0, 1, 2))


class Rows(unittest.TestCase):
    def test_a_blend_is_the_weighted_sum_rounded_to_float32_once(self) -> None:
        voices = array("f", [1.0] * ROW_FLOATS + [3.0] * ROW_FLOATS)
        out = blend_rows(voices, [(0, 0.6), (1, 0.4)])
        self.assertEqual(len(out), ROW_FLOATS)
        self.assertEqual(set(out), {array("f", [0.6 * 1.0 + 0.4 * 3.0])[0]})


class Build(unittest.TestCase):
    def setUp(self) -> None:
        self.tmp = tempfile.TemporaryDirectory()
        self.dir = Path(self.tmp.name)
        self.stock = self.dir / "voices.bin"
        self.stock.write_bytes(voices_bytes())
        self.recipe = self.dir / "malakor.toml"
        self.recipe.write_text(RECIPE, encoding="utf-8")

    def tearDown(self) -> None:
        self.tmp.cleanup()

    def test_the_same_inputs_give_the_same_bytes(self) -> None:
        one = build(self.stock, self.recipe, self.dir / "a.bin")
        two = build(self.stock, self.recipe, self.dir / "b.bin")
        a, b = (self.dir / "a.bin").read_bytes(), (self.dir / "b.bin").read_bytes()
        self.assertEqual(a, b)
        self.assertEqual(one["output"]["sha256"], hashlib.sha256(a).hexdigest())
        self.assertEqual(one["voices"], two["voices"])

    def test_the_file_keeps_its_size_and_only_the_slots_change(self) -> None:
        meta = build(self.stock, self.recipe, self.dir / "out.bin")
        stock, out = self.stock.read_bytes(), (self.dir / "out.bin").read_bytes()
        self.assertEqual(len(out), len(stock))
        slots = {v["sid"] for v in meta["voices"]}
        self.assertEqual(slots, {52, 51})
        for sid in range(SPEAKERS):
            if sid not in slots:
                self.assertEqual(speaker(out, sid), speaker(stock, sid), sid)
        onyx, lewis = speaker(stock, 17), speaker(stock, 27)
        want = array("f", [0.6 * o + 0.4 * w for o, w in zip(onyx, lewis)])
        self.assertEqual(speaker(out, 52), want)

    def test_the_sidecar_json_names_each_blend_and_hashes_its_inputs(self) -> None:
        build(self.stock, self.recipe, self.dir / "out.bin")
        meta = json.loads((self.dir / "out.bin.json").read_text(encoding="utf-8"))
        self.assertEqual(
            meta["stock"]["sha256"], hashlib.sha256(self.stock.read_bytes()).hexdigest()
        )
        self.assertEqual(
            meta["recipe"]["sha256"], hashlib.sha256(self.recipe.read_bytes()).hexdigest()
        )
        first = meta["voices"][0]
        self.assertEqual(first["name"], "malakor-a")
        self.assertEqual(first["sid"], 52)
        self.assertEqual(first["replaces"], "zm_yunyang")
        self.assertEqual(first["recipe"], {"am_onyx": 0.6, "bm_lewis": 0.4})
        self.assertNotIn(str(self.dir), json.dumps(meta))  # file names only, never a path

    def test_a_file_of_another_model_is_refused(self) -> None:
        self.stock.write_bytes(voices_bytes(SPEAKERS - 1))
        with self.assertRaises(BlendError):
            build(self.stock, self.recipe, self.dir / "out.bin")
        self.stock.write_bytes(b"\x00" * 12)
        with self.assertRaises(BlendError):
            build(self.stock, self.recipe, self.dir / "out.bin")

    def test_the_committed_recipe_parses_into_two_original_blends(self) -> None:
        import tomllib

        committed = Path(__file__).resolve().parent.parent / "recipes" / "malakor.toml"
        blends = assign_slots(load_recipes(tomllib.loads(committed.read_text(encoding="utf-8"))))
        self.assertEqual(len(blends), 2)
        for b in blends:
            self.assertAlmostEqual(sum(w for _, w in b.weights), 1.0)
            self.assertNotEqual(b.sid, STOCK_SID)


if __name__ == "__main__":
    unittest.main()
