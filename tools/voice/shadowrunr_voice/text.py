# SPDX-License-Identifier: Apache-2.0
"""Where a line's sentences end, and how they are cut for a fast first audio.

The engine renders one sentence per process run, so the first audio arrives after one sentence's
work, not the whole line's. :func:`sentences` owns the boundary rules: a decimal, an abbreviation
("Dr.", "e.g.") or a stop before a lowercase word does not end a sentence, and closing quotes and
brackets stay with the sentence they close. :func:`split_line` folds a fragment of a few
characters into the sentence before it (a process run costs a model load), and cuts a long first
sentence at a comma so the first audio starts early.
"""

from __future__ import annotations

import re

__all__ = ["FIRST_MAX", "FOLD_UNDER", "first_cut", "sentences", "split_line"]

#: A first sentence longer than this is cut at a comma, when it has one in a usable place.
FIRST_MAX = 120
#: A sentence shorter than this joins the one before it: half a second of audio is not worth a
#: model load of its own.
FOLD_UNDER = 12

_CLOSERS = "\"')]’”"
_BOUNDARY = re.compile(r"[.!?…]+[\"')\]’”]*\s+")
_LAST_WORD = re.compile(r"(\S+)$")
_ABBREVIATIONS = frozenset(
    {"dr", "mr", "mrs", "ms", "prof", "st", "jr", "sr", "vs", "e.g", "i.e", "cf", "approx"}
)


def _ends_sentence(text: str, match: re.Match[str]) -> bool:
    stop = match.group(0).rstrip()
    if not stop.rstrip(_CLOSERS).endswith("."):
        return True
    following = text[match.end() : match.end() + 1]
    if following.islower():
        return False
    word = _LAST_WORD.search(text[: match.start()])
    return not (word and word.group(1).lstrip("(\"'‘“").lower() in _ABBREVIATIONS)


def sentences(text: str) -> list[str]:
    """``text`` cut where a sentence ends, blanks dropped. A run with no stop is one sentence.

    Runs of whitespace, newlines included, become one space: a sentence is one engine argument."""
    text = " ".join(text.split())
    out: list[str] = []
    start = 0
    for match in _BOUNDARY.finditer(text):
        if _ends_sentence(text, match):
            out.append(text[start : match.end()].strip())
            start = match.end()
    out.append(text[start:].strip())
    return [sentence for sentence in out if sentence]


def first_cut(sentence: str, limit: int = FIRST_MAX) -> list[str]:
    """``sentence`` as one piece, or two cut at a comma when it is longer than ``limit``.

    The cut is the last comma inside the first ``limit`` characters that leaves at least
    :data:`FOLD_UNDER` characters on each side; failing that, the first such comma anywhere. A
    sentence with no usable comma is left whole."""
    if len(sentence) <= limit:
        return [sentence]
    usable = [
        at
        for at, char in enumerate(sentence)
        if char == ","
        and at + 1 >= FOLD_UNDER
        and len(sentence) - at - 1 >= FOLD_UNDER
    ]
    if not usable:
        return [sentence]
    inside = [at for at in usable if at < limit]
    at = inside[-1] if inside else usable[0]
    return [sentence[: at + 1].strip(), sentence[at + 1 :].strip()]


def split_line(text: str) -> list[str]:
    """The line as the engine renders it: sentences, short ones folded, a long first one cut."""
    out: list[str] = []
    for part in sentences(text):
        if out and len(part) < FOLD_UNDER:
            out[-1] = f"{out[-1]} {part}"
        else:
            out.append(part)
    if out:
        out[:1] = first_cut(out[0])
    return out
