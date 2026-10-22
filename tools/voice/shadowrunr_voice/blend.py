# SPDX-License-Identifier: Apache-2.0
"""An original voice for Malakor: stock Kokoro voices (Apache-2.0) mixed by weight, no cloning.

``python -m shadowrunr_voice.blend --voices <stock voices.bin> --recipe <recipe.toml>``
``[--out <file>]``
writes a copy of the stock ``voices.bin`` with each recipe's blend in a speaker slot of its own,
and beside it ``<file>.json``: each blend's sid, its normalised recipe, the stock speaker it
displaced, and the sha256 of the stock file, the recipe and the output. The file is never the
repo's: ``--out`` defaults to ``~/.shadowrunr/voices/<recipe>-voices.bin`` (``malakor.toml`` makes
``malakor-voices.bin``), the folder the daemon looks in for a ``[voice] voices_file`` name.

**The file.** ``voices.bin`` is raw little-endian float32, ``[speakers, 510, 256]``: per speaker,
row ``r`` is the style for an ``r``-token utterance. A blend is the weighted sum of its inputs,
row by row, summed in double precision and rounded to float32 once, so the same inputs give the
same bytes.

**Why a slot is replaced, not appended.** sherpa-onnx takes the speaker count from the model's
metadata (53 for kokoro-multi-lang-v1_0) and refuses a voices file of any other size ("Corrupted
--kokoro-voices ... Expected #floats: 6919680"). So a blend takes the slot of a stock speaker the
show does not use: by default the last sids, the Mandarin male voices, counting down. The engine
and the mouth shapes' durations graph then read the same file and the same sid.

**The speaker names.** :data:`KOKORO_V1_SPEAKERS` is the model's own: ``model.onnx`` of
kokoro-multi-lang-v1_0 carries its speaker names in its metadata, and sherpa-onnx prints them as
``0->af_alloy, 1->af_aoede, ...`` when it loads with ``debug`` on (sherpa's export packs
hexgrad/Kokoro-82M's ``voices/*.pt`` in sorted name order). ``af_heart`` is sid 3, the sid the
sidecar already runs. Rendered pitch agrees: the ``am_*``/``bm_*`` sids come out lower than the
``af_*``/``bf_*`` ones, ``am_onyx`` lowest.

Standard library only (``array``, ``tomllib``), so it runs in the sidecar's own interpreter.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import sys
import tomllib
from array import array
from collections.abc import Mapping, Sequence
from dataclasses import dataclass
from pathlib import Path
from typing import Any

__all__ = [
    "KOKORO_V1_SPEAKERS",
    "ROW_FLOATS",
    "Blend",
    "BlendError",
    "assign_slots",
    "blend_rows",
    "build",
    "default_out",
    "load_recipes",
    "sid_of",
]

STYLE_ROWS = 510
STYLE_DIM = 256
#: Floats per speaker: 510 style rows of 256.
ROW_FLOATS = STYLE_ROWS * STYLE_DIM

#: sherpa-onnx's kokoro-multi-lang-v1_0 speakers, in sid order.
KOKORO_V1_SPEAKERS: tuple[str, ...] = (
    "af_alloy", "af_aoede", "af_bella", "af_heart", "af_jessica", "af_kore", "af_nicole",
    "af_nova", "af_river", "af_sarah", "af_sky", "am_adam", "am_echo", "am_eric", "am_fenrir",
    "am_liam", "am_michael", "am_onyx", "am_puck", "am_santa", "bf_alice", "bf_emma",
    "bf_isabella", "bf_lily", "bm_daniel", "bm_fable", "bm_george", "bm_lewis", "ef_dora",
    "em_alex", "ff_siwis", "hf_alpha", "hf_beta", "hm_omega", "hm_psi", "if_sara", "im_nicola",
    "jf_alpha", "jf_gongitsune", "jf_nezumi", "jf_tebukuro", "jm_kumo", "pf_dora", "pm_alex",
    "pm_santa", "zf_xiaobei", "zf_xiaoni", "zf_xiaoxiao", "zf_xiaoyi", "zm_yunjian", "zm_yunxi",
    "zm_yunxia", "zm_yunyang",
)  # fmt: skip
#: The sidecar's stock voice: never displaced.
STOCK_SID = 3
#: Where blended voices files live by default, outside the repo; the daemon's ``voices_dir``.
VOICES_DIR = Path.home() / ".shadowrunr" / "voices"


class BlendError(ValueError):
    """A recipe or a voices file that cannot be blended; the message says which and why."""


@dataclass(frozen=True)
class Blend:
    """One recipe: its name, its inputs by sid with weights summing to 1, and its slot."""

    name: str
    weights: tuple[tuple[int, float], ...]
    sid: int = -1

    def recipe(self) -> dict[str, float]:
        return {KOKORO_V1_SPEAKERS[sid]: weight for sid, weight in self.weights}


def sid_of(name: str) -> int:
    try:
        return KOKORO_V1_SPEAKERS.index(name)
    except ValueError:
        raise BlendError(f"{name!r} is not a kokoro-multi-lang-v1_0 speaker") from None


def load_recipes(table: Mapping[str, Any]) -> list[Blend]:
    """``{"voices": {name: {speaker: weight, ...}}}`` (a parsed recipe file) to blends, weights
    normalised to sum to 1, in the file's order. A blend may set ``sid`` to pick its slot."""
    voices = table.get("voices")
    if not isinstance(voices, Mapping) or not voices:
        raise BlendError("the recipe has no [voices] table")
    out: list[Blend] = []
    for name, mix in voices.items():
        if not isinstance(mix, Mapping):
            raise BlendError(f"{name}: a blend is a table of speaker = weight")
        slot = mix.get("sid", -1)
        parts = [(k, v) for k, v in mix.items() if k != "sid"]
        if not parts:
            raise BlendError(f"{name}: no speakers")
        if any(not isinstance(v, (int, float)) or isinstance(v, bool) or v <= 0 for _, v in parts):
            raise BlendError(f"{name}: every weight is a positive number")
        total = float(sum(v for _, v in parts))
        weights = tuple((sid_of(k), float(v) / total) for k, v in parts)
        if len({sid for sid, _ in weights}) != len(weights):
            raise BlendError(f"{name}: a speaker appears twice")
        if not isinstance(slot, int) or isinstance(slot, bool):
            raise BlendError(f"{name}: sid is an integer")
        out.append(Blend(name, weights, slot))
    return out


def assign_slots(blends: Sequence[Blend], speakers: int = len(KOKORO_V1_SPEAKERS)) -> list[Blend]:
    """Each blend's slot: its own ``sid`` when set, otherwise the highest free sid. Never the
    stock voice, never a speaker some blend reads, never two blends in one slot."""
    inputs = {sid for b in blends for sid, _ in b.weights}
    taken: set[int] = set()
    for b in blends:
        if b.sid >= 0:
            if b.sid >= speakers:
                raise BlendError(f"{b.name}: sid {b.sid} is past the {speakers} speakers")
            if b.sid == STOCK_SID or b.sid in inputs or b.sid in taken:
                raise BlendError(f"{b.name}: sid {b.sid} is not free to replace")
            taken.add(b.sid)
    free = [s for s in range(speakers - 1, -1, -1) if s not in inputs | taken | {STOCK_SID}]
    out: list[Blend] = []
    for b in blends:
        if b.sid >= 0:
            out.append(b)
            continue
        if not free:
            raise BlendError("no speaker slot is left for the blends")
        out.append(Blend(b.name, b.weights, free.pop(0)))
    return out


def blend_rows(voices: array[float], weights: Sequence[tuple[int, float]]) -> array[float]:
    """One speaker's ``510 x 256`` floats: the weighted sum of the inputs, in double precision,
    rounded to float32 once."""
    acc = [0.0] * ROW_FLOATS
    for sid, weight in weights:
        base = sid * ROW_FLOATS
        src = voices[base : base + ROW_FLOATS]
        acc = [a + weight * s for a, s in zip(acc, src)]
    return array("f", acc)


def _read_voices(path: Path) -> array[float]:
    data = array("f")
    raw = path.read_bytes()
    if len(raw) % (ROW_FLOATS * 4):
        raise BlendError(f"{path.name} is not a whole number of 510 x 256 float32 speakers")
    data.frombytes(raw)
    if sys.byteorder != "little":
        data.byteswap()
    return data


def default_out(recipe_path: Path, folder: Path = VOICES_DIR) -> Path:
    """``<folder>/<recipe stem>-voices.bin``: what a character's ``[voice] voices_file`` names."""
    return folder / f"{recipe_path.stem}-voices.bin"


def build(voices_path: Path, recipe_path: Path, out_path: Path) -> dict[str, Any]:
    """Write the blended voices file and its ``.json``; returns what the ``.json`` holds."""
    voices = _read_voices(voices_path)
    speakers = len(voices) // ROW_FLOATS
    if speakers != len(KOKORO_V1_SPEAKERS):
        raise BlendError(
            f"{voices_path.name} has {speakers} speakers; kokoro-multi-lang-v1_0 has "
            f"{len(KOKORO_V1_SPEAKERS)}"
        )
    recipe_bytes = recipe_path.read_bytes()
    try:
        table = tomllib.loads(recipe_bytes.decode("utf-8"))
    except (UnicodeDecodeError, tomllib.TOMLDecodeError) as exc:
        raise BlendError(f"{recipe_path.name} is not TOML: {exc}") from None
    blends = assign_slots(load_recipes(table), speakers)
    out = array("f", voices)
    for b in blends:
        start = b.sid * ROW_FLOATS
        out[start : start + ROW_FLOATS] = blend_rows(voices, b.weights)
    if sys.byteorder != "little":
        out.byteswap()
    body = out.tobytes()
    out_path.parent.mkdir(parents=True, exist_ok=True)
    out_path.write_bytes(body)
    meta: dict[str, Any] = {
        "model": "kokoro-multi-lang-v1_0",
        "stock": {"file": voices_path.name, "sha256": hashlib.sha256(voices.tobytes()).hexdigest()},
        "recipe": {"file": recipe_path.name, "sha256": hashlib.sha256(recipe_bytes).hexdigest()},
        "output": {"file": out_path.name, "sha256": hashlib.sha256(body).hexdigest()},
        "voices": [
            {
                "name": b.name,
                "sid": b.sid,
                "replaces": KOKORO_V1_SPEAKERS[b.sid],
                "recipe": b.recipe(),
            }
            for b in blends
        ],
    }
    out_path.with_name(out_path.name + ".json").write_text(
        json.dumps(meta, indent=2) + "\n", encoding="utf-8"
    )
    return meta


def main(argv: Sequence[str] | None = None) -> int:
    parser = argparse.ArgumentParser(prog="shadowrunr_voice.blend", description=__doc__)
    parser.add_argument("--voices", required=True, type=Path, help="the stock voices.bin")
    parser.add_argument("--recipe", required=True, type=Path, help="a recipe .toml")
    parser.add_argument(
        "--out",
        type=Path,
        default=None,
        help="the blended voices file (default ~/.shadowrunr/voices/<recipe>-voices.bin)",
    )
    args = parser.parse_args(argv)
    out = args.out if args.out is not None else default_out(args.recipe, VOICES_DIR)
    try:
        meta = build(args.voices, args.recipe, out)
    except (BlendError, OSError) as exc:
        print(f"blend: {exc}", file=sys.stderr)
        return 1
    for v in meta["voices"]:
        print(f"{v['name']}: sid {v['sid']} (replaces {v['replaces']}) {v['recipe']}")
    print(f"wrote {out.name}, sha256 {meta['output']['sha256']}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
