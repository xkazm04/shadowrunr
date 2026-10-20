# SPDX-License-Identifier: Apache-2.0
from __future__ import annotations

import unittest

from shadowrunr_voice.text import FIRST_MAX, first_cut, sentences, split_line


class Sentences(unittest.TestCase):
    def test_cuts_at_stops(self) -> None:
        self.assertEqual(
            sentences("I am Malakor. Kneel! Will you not?"),
            ["I am Malakor.", "Kneel!", "Will you not?"],
        )

    def test_no_stop_is_one_sentence(self) -> None:
        self.assertEqual(sentences("the crypt hums"), ["the crypt hums"])

    def test_blank_is_nothing(self) -> None:
        self.assertEqual(sentences("   \n "), [])

    def test_abbreviation_decimal_and_lowercase_do_not_end_one(self) -> None:
        self.assertEqual(
            sentences("Ask Dr. Vex about it. It costs 3.5 souls, e.g. yours. Then go."),
            ["Ask Dr. Vex about it.", "It costs 3.5 souls, e.g. yours.", "Then go."],
        )

    def test_closers_stay_with_their_sentence(self) -> None:
        self.assertEqual(
            sentences('He said "bow." Then nothing.'), ['He said "bow."', "Then nothing."]
        )

    def test_newlines_become_spaces(self) -> None:
        self.assertEqual(sentences("One line\nand more.\nNext."), ["One line and more.", "Next."])


class SplitLine(unittest.TestCase):
    def test_short_fragment_folds_into_the_one_before(self) -> None:
        self.assertEqual(
            split_line("The tithe is due tonight. Pay. Or burn."),
            ["The tithe is due tonight. Pay. Or burn."],
        )

    def test_short_first_sentence_stands(self) -> None:
        self.assertEqual(split_line("Hi. The tithe is due."), ["Hi.", "The tithe is due."])

    def test_long_first_sentence_is_cut_at_a_comma(self) -> None:
        first = (
            "Mortals of the chat, you have come to my crypt with your little questions and your "
            "littler offerings, and I have weighed every one of them against the dark"
        )
        self.assertGreater(len(first), FIRST_MAX)
        parts = split_line(first + ". Kneel now, mortals.")
        self.assertEqual(len(parts), 3)
        self.assertTrue(parts[0].endswith("offerings,"))
        self.assertLessEqual(len(parts[0]), FIRST_MAX)
        self.assertEqual(" ".join(parts), first + ". Kneel now, mortals.")

    def test_only_the_first_sentence_is_cut(self) -> None:
        long = "Word, " + "word, " * 39 + "end."
        parts = split_line("Short opener here. " + long)
        self.assertEqual(parts, ["Short opener here.", long.strip()])

    def test_long_sentence_with_no_comma_stays_whole(self) -> None:
        long = "word " * 40 + "end."
        self.assertEqual(first_cut(long.strip()), [long.strip()])

    def test_a_comma_too_close_to_an_edge_is_not_used(self) -> None:
        long = "Yes, " + "x" * 130 + "."
        self.assertEqual(first_cut(long), [long])

    def test_falls_back_to_the_first_comma_past_the_limit(self) -> None:
        long = "y" * 130 + ", and then the rest of it."
        self.assertEqual(first_cut(long), ["y" * 130 + ",", "and then the rest of it."])


if __name__ == "__main__":
    unittest.main()
