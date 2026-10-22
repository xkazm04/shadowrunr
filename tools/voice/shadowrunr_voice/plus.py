# SPDX-License-Identifier: Apache-2.0
"""Kokoro Plus: a line's delivery cue (``say.delivery``) turned into the levers Kokoro has.

The voice lab (2026-10-22, the owner's pick for development) proved four levers. Each is
driven here from the cue ``{emotion, intensity, pace}`` alone, by the tables below:

- **Speed.** One speed per line: :data:`PACE_SPEED` for the pace, plus the emotion's nudge,
  held inside :data:`SPEED_RANGE`. The engine and the durations graph behind the mouth shapes
  get the same speed, so the shapes still line up.
- **Emotion voice.** At an intensity of :data:`VOICE_FLOOR` or more the line is spoken by its
  emotion's blend: a slot of the same voices file, written by :mod:`.blend` from the recipe's
  ``[emotions]`` table and listed in the file's ``.json``. Below it, and for an emotion with no
  blend, the line keeps the base voice.
- **Pauses.** Silence after every sentence but the last, and at slow pace after every clause
  (a comma, semicolon or colon) too. The line is cut there into parts, each rendered on its
  own. Kokoro's own quiet at an edge that borders a pause is cut to :data:`EDGE_KEEP_S`, as
  the lab did, so the pause is the plan's; the silence is added to the end of a part before
  the FX chain, so the crypt rings into it, and the part's mouth shapes end in ``sil`` over
  it.
- **Pitch.** The emotion's semitones, plus the blend's register correction when the blend
  speaks, added to the FX preset's own shift in its one STFT pass (the length is kept). A total
  past :data:`PITCH_FLOOR_ST` is held there: the phase vocoder smears below it.

Pauses and the emotion's semitones are given at full intensity and scale with it: the lab's
values divided by the intensity the lab line was directed at, so the bench lines (which carry
the lab's intensities) get the lab's values back. A say without ``delivery`` never comes here:
it renders exactly as before.

Standard library only: the sidecar parses and plans without numpy.
"""

from __future__ import annotations

import json
import re
import sys
from array import array
from collections.abc import Mapping
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any

from .text import first_cut, sentences

__all__ = [
    "EMOTIONS",
    "LEVERS",
    "PACES",
    "PACE_SPEED",
    "PITCH_FLOOR_ST",
    "VOICE_FLOOR",
    "Delivery",
    "EmotionVoice",
    "Lever",
    "Plan",
    "clamp_note",
    "clauses",
    "cut",
    "edges",
    "emotion_voices",
    "insert_silence",
    "parse_delivery",
    "plan_line",
    "trim_spans",
]

#: The closed cue set (``shadowrunr-types`` ``Emotion``, snake_case on the wire).
EMOTIONS = (
    "neutral", "menace", "glee", "deadpan", "hushed", "outrage", "sarcasm", "weary", "triumph",
    "tender",
)  # fmt: skip
PACES = ("slow", "normal", "fast")
#: The cue's intensity when a say leaves it out (the brain's partial cue ``[glee]``).
DEFAULT_INTENSITY = 0.6
#: Speed per pace, before the emotion's nudge.
PACE_SPEED = {"slow": 0.85, "normal": 1.0, "fast": 1.18}
#: Kokoro stays intelligible inside this.
SPEED_RANGE = (0.6, 1.6)
#: The intensity from which a line takes its emotion's blend.
VOICE_FLOOR = 0.4
#: The lowest total shift the FX pass is asked for (the heavy preset's own).
PITCH_FLOOR_ST = -4.0
#: Clause cuts at slow pace: after a comma, semicolon or colon followed by a space.
_CLAUSE = re.compile(r"(?<=[,;:])\s+")


@dataclass(frozen=True)
class Lever:
    """One emotion's levers: the speed nudge, and at full intensity its semitones and pauses."""

    nudge: float
    pitch_st: float
    sentence_ms: int
    clause_ms: int


#: Per emotion, from the voice lab's plus renders (``voice-lab/kokoro/README.md``). The pauses
#: and semitones there were directed at each line's intensity (menace 0.8, glee 0.9, deadpan
#: 0.6, hushed 0.7, outrage 1.0, sarcasm 0.8, weary 0.7, triumph 1.0, tender 0.6); here they
#: are at 1.0, the lab's value over that intensity, rounded to 5. The nudge is the lab's mean
#: speed against its pace's.
LEVERS: dict[str, Lever] = {
    "neutral": Lever(nudge=0.0, pitch_st=0.0, sentence_ms=150, clause_ms=150),
    "menace": Lever(nudge=-0.05, pitch_st=-1.25, sentence_ms=560, clause_ms=375),
    "glee": Lever(nudge=0.05, pitch_st=1.65, sentence_ms=145, clause_ms=135),
    "deadpan": Lever(nudge=-0.05, pitch_st=-0.85, sentence_ms=415, clause_ms=250),
    "hushed": Lever(nudge=0.05, pitch_st=0.0, sentence_ms=250, clause_ms=355),
    "outrage": Lever(nudge=-0.10, pitch_st=1.0, sentence_ms=205, clause_ms=150),
    "sarcasm": Lever(nudge=0.0, pitch_st=0.6, sentence_ms=440, clause_ms=440),
    "weary": Lever(nudge=0.0, pitch_st=-1.15, sentence_ms=430, clause_ms=355),
    "triumph": Lever(nudge=-0.04, pitch_st=1.2, sentence_ms=275, clause_ms=200),
    "tender": Lever(nudge=0.0, pitch_st=-0.5, sentence_ms=540, clause_ms=665),
}


@dataclass(frozen=True)
class Delivery:
    emotion: str = "neutral"
    intensity: float = DEFAULT_INTENSITY
    pace: str = "normal"

    def label(self) -> str:
        return f"{self.emotion} {self.intensity:.2f} {self.pace}"


def parse_delivery(raw: Any) -> Delivery | None:
    """A say's ``delivery``, or ``None`` when there is none (absent, null, not an object).

    Lenient as the wire is additive: an unknown emotion is neutral, an unknown pace normal, a
    missing or non-numeric intensity :data:`DEFAULT_INTENSITY`, and the intensity is clamped to
    ``0..1``."""
    if not isinstance(raw, Mapping):
        return None
    emotion = raw.get("emotion")
    if not isinstance(emotion, str) or emotion not in EMOTIONS:
        emotion = "neutral"
    pace = raw.get("pace")
    if not isinstance(pace, str) or pace not in PACES:
        pace = "normal"
    intensity = raw.get("intensity")
    if not isinstance(intensity, (int, float)) or isinstance(intensity, bool):
        value = DEFAULT_INTENSITY
    elif intensity != intensity:  # NaN
        value = DEFAULT_INTENSITY
    else:
        value = min(max(float(intensity), 0.0), 1.0)
    return Delivery(emotion, value, pace)


# --- the emotion voices ----------------------------------------------------------------------


@dataclass(frozen=True)
class EmotionVoice:
    """An emotion's blend in the voices file: its sid, and the semitones that put it back in
    the base voice's register."""

    sid: int
    correction_st: float = 0.0


def emotion_voices(voices: Path, base_sid: int, speakers: int = 53) -> dict[str, EmotionVoice]:
    """The emotion blends of ``base_sid`` in the voices file, from the ``.json`` beside it
    (:mod:`.blend`'s); empty when there is none, it is not readable, or its blends are of
    another voice. A blend of another base would change who speaks, so it is never used."""
    meta_path = voices.with_name(voices.name + ".json")
    try:
        meta = json.loads(meta_path.read_text(encoding="utf-8"))
    except (OSError, ValueError):
        return {}
    if not isinstance(meta, dict):
        return {}
    out: dict[str, EmotionVoice] = {}
    for entry in meta.get("emotions") or []:
        if not isinstance(entry, dict):
            continue
        name, sid, base = entry.get("name"), entry.get("sid"), entry.get("base_sid")
        correction = entry.get("pitch_correction", 0.0)
        if name not in EMOTIONS or name == "neutral" or base != base_sid:
            continue
        if not isinstance(sid, int) or isinstance(sid, bool) or not 0 <= sid < speakers:
            continue
        if not isinstance(correction, (int, float)) or isinstance(correction, bool):
            correction = 0.0
        out[name] = EmotionVoice(sid, float(correction))
    return out


# --- the plan --------------------------------------------------------------------------------


@dataclass
class Plan:
    """How one line is spoken: its parts, the silence after each, and one sid, speed and pitch
    for all of them."""

    delivery: Delivery
    sid: int
    speed: float
    #: Semitones added to the FX preset's own shift (0 with no FX chain).
    pitch_st: float
    parts: list[str] = field(default_factory=list)
    #: Milliseconds of silence after each part; the last is 0.
    pauses_ms: list[int] = field(default_factory=list)
    #: The total shift was held at :data:`PITCH_FLOOR_ST`.
    clamped: bool = False

    def describe(self) -> str:
        held = ", held" if self.clamped else ""
        return (
            f"{self.delivery.label()}: sid {self.sid}, speed {self.speed:.2f}, pitch "
            f"{self.pitch_st:+.2f} st{held}, pauses {self.pauses_ms}"
        )


def clauses(sentence: str) -> list[str]:
    """``sentence`` cut after each comma, semicolon or colon that a space follows."""
    return [piece for piece in _CLAUSE.split(sentence) if piece.strip()]


def _parts(text: str, slow: bool) -> list[tuple[str, str]]:
    """The line's parts with what ends each: ``sentence``, ``clause`` or ``cut`` (the long
    first sentence's latency cut, which is no pause). Short sentences are not folded: the warm
    engine pays no load per part, and each one is a beat of its own."""
    out: list[tuple[str, str]] = []
    for i, sentence in enumerate(sentences(text)):
        if slow:
            pieces = clauses(sentence)
            out += [(p, "clause") for p in pieces[:-1]] + [(pieces[-1], "sentence")]
            continue
        pieces = first_cut(sentence) if i == 0 else [sentence]
        out += [(p, "cut") for p in pieces[:-1]] + [(pieces[-1], "sentence")]
    return out


def total_shift(
    lever: Lever, intensity: float, voice: EmotionVoice | None, preset_st: float
) -> tuple[float, bool]:
    """The FX pass's total semitones for a line, and whether it was held at the floor."""
    total = preset_st + lever.pitch_st * intensity + (voice.correction_st if voice else 0.0)
    if total < PITCH_FLOOR_ST:
        return PITCH_FLOOR_ST, True
    return total, False


def plan_line(
    text: str,
    delivery: Delivery,
    *,
    base_sid: int,
    voices: Mapping[str, EmotionVoice],
    preset_st: float | None,
) -> Plan:
    """The plan for ``text`` spoken with ``delivery``. ``preset_st`` is the FX preset's shift,
    or ``None`` with no FX chain (then there is no pitch lever)."""
    lever = LEVERS.get(delivery.emotion, LEVERS["neutral"])
    voice = voices.get(delivery.emotion) if delivery.intensity >= VOICE_FLOOR else None
    lo, hi = SPEED_RANGE
    speed = round(min(max(PACE_SPEED[delivery.pace] + lever.nudge, lo), hi), 3)
    pitch, clamped = 0.0, False
    if preset_st is not None:
        total, clamped = total_shift(lever, delivery.intensity, voice, preset_st)
        pitch = round(total - preset_st, 3)
    parts = _parts(text, delivery.pace == "slow")
    gap = {
        "sentence": round(lever.sentence_ms * delivery.intensity),
        "clause": round(lever.clause_ms * delivery.intensity),
        "cut": 0,
    }
    pauses = [gap[kind] for _, kind in parts]
    if pauses:
        pauses[-1] = 0
    return Plan(
        delivery=delivery,
        sid=voice.sid if voice is not None else base_sid,
        speed=speed,
        pitch_st=pitch,
        parts=[p for p, _ in parts],
        pauses_ms=pauses,
        clamped=clamped,
    )


def clamp_note(voices: Mapping[str, EmotionVoice], preset_st: float | None) -> list[str]:
    """The emotions whose total shift can pass :data:`PITCH_FLOOR_ST` at some intensity, which
    the hello names once. The shift is linear in the intensity, so its ends decide."""
    if preset_st is None:
        return []
    held: list[str] = []
    for name in EMOTIONS:
        lever = LEVERS[name]
        voice = voices.get(name)
        ends = [(0.0, None), (VOICE_FLOOR, None), (1.0, voice)]
        if voice is not None:
            ends.append((VOICE_FLOOR, voice))
        if any(total_shift(lever, i, v, preset_st)[1] for i, v in ends):
            held.append(name)
    return held


# --- the edges -------------------------------------------------------------------------------

#: Quiet at a pause's edge is cut down to this, so the pause is the plan's and not the plan's
#: plus Kokoro's own lead-in and tail (60 to 140 ms each, measured): the lab's 20 ms.
EDGE_KEEP_S = 0.02
#: The fade over a cut edge.
EDGE_FADE_S = 0.005
#: A quiet sample: the engine's own pause threshold (``|s| <= 327``, about 0.01 of full scale).
QUIET_LEVEL = 327


def _samples(pcm: bytes) -> array[int]:
    audio = array("h")
    audio.frombytes(pcm[: len(pcm) - len(pcm) % 2])
    if sys.byteorder != "little":
        audio.byteswap()
    return audio


def edges(pcm: bytes, sample_rate: int, *, lead: bool, tail: bool) -> tuple[int, int]:
    """The ``[a, b)`` samples of a part kept when its quiet lead-in (``lead``) or tail
    (``tail``) borders a pause: all but :data:`EDGE_KEEP_S` of it is cut. All quiet, or neither
    edge asked for, keeps it whole."""
    audio = _samples(pcm)
    n = len(audio)
    first = next((i for i, s in enumerate(audio) if abs(s) > QUIET_LEVEL), None)
    if first is None or not (lead or tail):
        return 0, n
    last = next(i for i in range(n - 1, -1, -1) if abs(audio[i]) > QUIET_LEVEL)
    keep = round(EDGE_KEEP_S * sample_rate)
    a = max(first - keep, 0) if lead else 0
    b = min(last + 1 + keep, n) if tail else n
    return a, b


def cut(pcm: bytes, a: int, b: int, sample_rate: int) -> bytes:
    """Samples ``[a, b)`` of ``pcm``, with a short fade in where the start was cut and out where
    the end was."""
    audio = _samples(pcm)
    n = len(audio)
    out = audio[a:b]
    fade = min(round(EDGE_FADE_S * sample_rate), len(out) // 2)
    if fade > 0:
        for i in range(fade):
            gain = i / fade
            if a > 0:
                out[i] = round(out[i] * gain)
            if b < n:
                out[len(out) - 1 - i] = round(out[len(out) - 1 - i] * gain)
    if sys.byteorder != "little":
        out.byteswap()
    return out.tobytes()


# --- the mouth shapes ------------------------------------------------------------------------


def trim_spans(spans: list[list[Any]], a: int, b: int) -> list[list[Any]]:
    """``[[v, start, end]]`` sample spans cut to ``[a, b)`` and moved to start at 0: what the
    mouth does over the part that is kept."""
    out: list[list[Any]] = []
    for v, start, end in spans:
        lo, hi = max(start, a), min(end, b)
        if hi > lo:
            out.append([v, lo - a, hi - a])
    return out


def insert_silence(spans: list[list[Any]], at: int, n: int) -> list[list[Any]]:
    """``[[v, start, end]]`` sample spans with ``n`` samples of ``sil`` inserted at ``at``:
    spans after it move later by ``n``, a span across it is split around it, and a ``sil``
    beside the gap joins it. The timeline still covers the audio end to end."""
    if n <= 0:
        return [list(s) for s in spans]
    out: list[list[Any]] = []

    def put(v: str, a: int, b: int) -> None:
        if b <= a:
            return
        if out and out[-1][0] == v == "sil" and out[-1][2] == a:
            out[-1][2] = b
        else:
            out.append([v, a, b])

    placed = False
    for v, a, b in spans:
        if not placed and a >= at:
            put("sil", at, at + n)
            placed = True
        if b <= at or placed:
            put(v, a + (n if placed else 0), b + (n if placed else 0))
            continue
        put(v, a, at)
        put("sil", at, at + n)
        placed = True
        put(v, at + n, b + n)
    if not placed:
        put("sil", at, at + n)
    return out
