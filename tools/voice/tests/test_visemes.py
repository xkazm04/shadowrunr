# SPDX-License-Identifier: Apache-2.0
"""The pure part of the mouth shapes: hand-made tokens and durations, no numpy, no onnxruntime."""

from __future__ import annotations

import math
import tempfile
import unittest
from array import array
from pathlib import Path

from shadowrunr_voice.visemes import (
    FRAME,
    OCULUS,
    load_symbols,
    merge_short,
    to_ms,
    visemes_from_frames,
    visemes_of,
)

RATE = 24_000
# tokens.txt ids for the hand-made sentence " mˈɑp" ("mop", stressed), and a space.
SYMBOLS = {16: " ", 1: "m", 2: "ˈ", 3: "ɑ", 4: "p", 5: "h", 6: "a", 7: "ɪ"}


def tone(samples: int, level: int = 8000) -> bytes:
    return array(
        "h", (int(level * math.sin(2 * math.pi * 220 * i / RATE)) for i in range(samples))
    ).tobytes()


class Visemes(unittest.TestCase):
    def build(self, batch: list[int], frames: list[int], pcm: bytes | None = None) -> list[list]:
        raw = sum(frames) * FRAME
        return visemes_from_frames(
            SYMBOLS,
            batches=[batch],
            frames=[frames],
            raw_samples=raw,
            time_map=[(0, raw, 0, raw)],
            pcm=tone(raw) if pcm is None else pcm,
            sample_rate=RATE,
        )

    def test_a_word_maps_to_its_shapes_and_covers_the_audio(self) -> None:
        # pad, m, stress, ɑ, p, pad: 0 + 3 + 0 + 6 + 3 + 0 frames
        vis = self.build([0, 1, 2, 3, 4, 0], [0, 3, 0, 6, 3, 0])
        names = [v for v, _, _ in vis]
        self.assertEqual(names, ["PP", "aa", "PP"])
        self.assertEqual(vis[0][1], 0)
        self.assertEqual(vis[-1][2], 12 * FRAME)
        for (_, _, end), (_, start, _) in zip(vis, vis[1:]):
            self.assertEqual(end, start)

    def test_a_quiet_boundary_is_silence(self) -> None:
        frames = [0, 4, 4, 4, 0]
        raw = sum(frames) * FRAME
        pcm = bytearray(tone(raw))
        # the space's span, moved earlier by the lead like every edge
        quiet = slice(1000 * 2, 3600 * 2)
        pcm[quiet] = bytes(len(pcm[quiet]))
        vis = self.build([0, 3, 16, 3, 0], frames, bytes(pcm))
        self.assertIn("sil", [v for v, _, _ in vis])

    def test_a_diphthong_splits_and_h_borrows_the_next_vowel(self) -> None:
        segs = visemes_of(
            [{"p": "h", "a": 0, "b": 1000}, {"p": "aɪ", "a": 1000, "b": 3000}]
        )
        self.assertEqual([s["v"] for s in segs], ["aa", "aa", "ih"])
        self.assertEqual(segs[1]["b"], 1000 + int(2000 * 0.6))

    def test_no_segment_is_shorter_than_forty_ms_unless_it_is_a_closure(self) -> None:
        segs = [
            {"v": "aa", "a": 0, "b": 4800},
            {"v": "DD", "a": 4800, "b": 5000},
            {"v": "E", "a": 5000, "b": 9600},
        ]
        merged = merge_short(segs, RATE)
        self.assertTrue(all(s["b"] - s["a"] >= 0.04 * RATE for s in merged))
        closure = merge_short(
            [{"v": "aa", "a": 0, "b": 4800}, {"v": "PP", "a": 4800, "b": 5000},
             {"v": "E", "a": 5000, "b": 9600}],
            RATE,
        )  # fmt: skip
        pp = next(s for s in closure if s["v"] == "PP")
        self.assertEqual(pp["b"] - pp["a"], round(0.04 * RATE))

    def test_durations_that_do_not_cover_the_audio_are_refused(self) -> None:
        with self.assertRaises(ValueError):
            visemes_from_frames(
                SYMBOLS,
                batches=[[0, 3, 0]],
                frames=[[0, 3, 0]],
                raw_samples=5 * FRAME,
                time_map=[(0, 5 * FRAME, 0, 5 * FRAME)],
                pcm=tone(5 * FRAME),
                sample_rate=RATE,
            )

    def test_a_cut_pause_moves_the_later_shapes_earlier(self) -> None:
        frames = [0, 4, 0]
        raw = 4 * FRAME
        played = raw // 2
        vis = visemes_from_frames(
            SYMBOLS,
            batches=[[0, 3, 0]],
            frames=[frames],
            raw_samples=raw,
            time_map=[(0, raw, 0, played)],
            pcm=tone(played),
            sample_rate=RATE,
        )
        self.assertEqual(vis[-1][2], played)


class Milliseconds(unittest.TestCase):
    def test_samples_to_ms(self) -> None:
        self.assertEqual(
            to_ms([["PP", 0, 2400], ["aa", 2400, 4800]], 4800, RATE),
            [["PP", 0, 100], ["aa", 100, 200]],
        )

    def test_shapes_that_miss_the_end_or_the_names_are_dropped(self) -> None:
        self.assertIsNone(to_ms([["PP", 0, 2400]], 4800, RATE))
        self.assertIsNone(to_ms([["XX", 0, 4800]], 4800, RATE))
        self.assertIsNone(to_ms([], 0, RATE))

    def test_the_names_are_the_oculus_fifteen(self) -> None:
        self.assertEqual(len(OCULUS), 15)


class Symbols(unittest.TestCase):
    def test_load_symbols_keeps_the_space(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "tokens.txt"
            path.write_text("$ 0\n 16\nm 1\nˈ 2\n", encoding="utf-8")
            self.assertEqual(load_symbols(path), {0: "$", 16: " ", 1: "m", 2: "ˈ"})


if __name__ == "__main__":
    unittest.main()
