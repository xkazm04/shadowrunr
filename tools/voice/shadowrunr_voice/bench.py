# SPDX-License-Identifier: Apache-2.0
"""The blind voice bench: Malakor's blend candidates, each with the lich chain light and heavy,
lettered so the owner picks by ear, plus the stock ``af_heart`` as an unlettered reference.

``python -m shadowrunr_voice.bench --engine-dir <dir> --durations <onnx> --out <dir>``

1. Builds the blended voices file from ``recipes/malakor.toml`` (:mod:`.blend`), outside the
   repo (``--voices-out``, default ``~/.shadowrunr/voices/malakor-voices.bin``).
2. Loads Kokoro the way a warm worker does (:func:`.warm._tts`), once on the stock voices and
   once on the blended ones, and times each line on each voice at ``--threads``, the voices
   interleaved, the median of ``--reps``: the synthesis real-time factor.
3. Renders each line on each blend with the face on, through a real warm worker
   (:func:`.warm.serve` in a process of its own, with ``--voices`` the blended file: sherpa
   prints the token ids to its native stderr alone), works out the mouth shapes
   through the sidecar's own :class:`.sidecar.Faces` on the blended sid, and checks the timeline
   ends where the audio does.
4. Runs :mod:`.fx` light and heavy over each blend's audio, timed, checking the length holds.
5. Writes ``<letter>-<n>.wav`` per line and ``<letter>.wav`` (the lines with gaps) per
   candidate, ``reference-*.wav`` for the stock voice, ``metrics.json``, and ``key.json``: which
   letter is which. The letters are shuffled; the mapping is written to ``key.json`` only and
   never printed.

It needs the warm venv (sherpa_onnx, numpy, onnxruntime). Nothing here runs in the tests.
"""

from __future__ import annotations

import argparse
import json
import os
import random
import statistics
import time
from collections.abc import Sequence
from pathlib import Path
from typing import Any

from . import blend, fx
from .engine import SAMPLE_RATE, SILENCE_SCALE, Rendered, paths_for, scale_silence
from .sidecar import Faces
from .visemes import Durations, load_symbols

#: Malakor's own lines from sandbox run ``sbx-20261021-143530-10m`` (fictional viewers): short,
#: a question, long.
LINES = (
    "Eleven centuries of dread, and I am being chased by a landlord.",
    "GloamCandle, you want me to storm a streamer's channel? Doors are beneath us, and my rules "
    "stay sealed no matter how many times you knock.",
    "GloamLantern42, Dread Council, you arrive and vote for the Inspector in one breath. "
    "Efficient. Terrifying. The vote has fifty seconds left, so don't get comfortable.",
)
PRESET_NAMES = ("light", "heavy")
STOCK_SID = blend.STOCK_SID
GAP_S = 0.7
RECIPE = Path(__file__).resolve().parent.parent / "recipes" / "malakor.toml"


def _config(sid: int, faced: bool) -> Any:
    import sherpa_onnx

    config = sherpa_onnx.GenerationConfig()
    config.sid = sid
    config.speed = 1.0
    config.silence_scale = 1.0 if faced else SILENCE_SCALE
    return config


def _pcm(audio: Any) -> bytes:
    import numpy as np

    samples = np.asarray(audio.samples, dtype=np.float32)
    return (np.clip(samples, -1.0, 1.0) * 32767).astype("<i2").tobytes()  # as the warm worker


def _seconds(pcm: bytes) -> float:
    return len(pcm) / 2 / SAMPLE_RATE


def time_synthesis(
    voices: dict[str, tuple[Any, int]], reps: int
) -> tuple[dict[str, list[dict[str, Any]]], dict[str, list[bytes]]]:
    """Each voice on each line: one warm-up, then ``reps`` timed renders with the voices
    interleaved. The RTF is render time over audio time, median of the reps."""
    timings: dict[str, list[list[float]]] = {k: [[] for _ in LINES] for k in voices}
    audio: dict[str, list[bytes]] = {k: [b""] * len(LINES) for k in voices}
    for tts, sid in voices.values():
        tts.generate(LINES[0], _config(sid, False))
    for rep in range(reps + 1):
        for i, line in enumerate(LINES):
            for name, (tts, sid) in voices.items():
                began = time.perf_counter()
                out = tts.generate(line, _config(sid, False))
                took = time.perf_counter() - began
                if rep == 0:
                    continue  # this line's own warm-up on this voice
                timings[name][i].append(took)
                audio[name][i] = _pcm(out)
    rows: dict[str, list[dict[str, Any]]] = {}
    for name, per_line in timings.items():
        rows[name] = []
        for i, took in enumerate(per_line):
            secs = _seconds(audio[name][i])
            med = statistics.median(took)
            rows[name].append(
                {"line": i + 1, "audio_s": round(secs, 3), "render_ms": round(med * 1000, 1),
                 "rtf": round(med / secs, 4)}
            )  # fmt: skip
    return rows, audio


def faced_render(worker: Any, line: str, faces: Faces) -> tuple[bytes, dict[str, Any]]:
    """One line with the face on, through a warm worker: the audio the sidecar would send, and
    its mouth-shape check."""
    raw, tokens = worker.render(line, 120.0)
    pcm, time_map = scale_silence(raw, SAMPLE_RATE)
    rendered = Rendered(pcm=pcm, tokens=tokens, raw_samples=len(raw) // 2, time_map=time_map)
    visemes = faces.visemes(rendered, SAMPLE_RATE)
    samples = len(pcm) // 2
    end_ms = round(samples * 1000 / SAMPLE_RATE)
    check = {
        "batches": len(rendered.tokens),
        "visemes": len(visemes) if visemes else 0,
        "audio_ms": end_ms,
        "timeline_end_ms": visemes[-1][2] if visemes else None,
        "ok": bool(visemes) and visemes[-1][2] == end_ms,
    }
    return pcm, check


def time_fx(pcm: bytes, preset: str, reps: int) -> tuple[bytes, float]:
    """The chain on one sentence: its output and the median seconds it took."""
    wet = fx.apply(pcm, preset)
    took = []
    for _ in range(reps):
        began = time.perf_counter()
        wet = fx.apply(pcm, preset)
        took.append(time.perf_counter() - began)
    if len(wet) != len(pcm):
        raise RuntimeError(f"fx {preset} changed the length: {len(pcm)} -> {len(wet)}")
    return wet, statistics.median(took)


def _joined(parts: Sequence[bytes]) -> bytes:
    gap = b"\x00\x00" * int(GAP_S * SAMPLE_RATE)
    return gap.join(parts)


def run(args: argparse.Namespace) -> dict[str, Any]:
    from . import warm

    engine_dir = Path(args.engine_dir)
    stock_paths = paths_for(engine_dir)
    meta = blend.build(stock_paths.voices, Path(args.recipe), Path(args.voices_out))
    blends = [(v["name"], int(v["sid"])) for v in meta["voices"]]
    blend_paths = paths_for(engine_dir, Path(args.voices_out))
    out = Path(args.out)
    out.mkdir(parents=True, exist_ok=True)

    stock = warm._tts(stock_paths, args.threads, False)
    blended = warm._tts(blend_paths, args.threads, False)
    voices = {"af_heart": (stock, STOCK_SID)} | {name: (blended, sid) for name, sid in blends}
    synthesis, audio = time_synthesis(voices, args.reps)
    voices.clear()
    del stock, blended
    for i, pcm in enumerate(audio["af_heart"], 1):
        fx.write_wav(out / f"reference-{i}.wav", pcm)
    fx.write_wav(out / "reference.wav", _joined(audio["af_heart"]))

    faces = Faces(
        Durations(Path(args.durations), blend_paths.voices), load_symbols(blend_paths.tokens), 0
    )
    candidates = [(name, sid, preset) for name, sid in blends for preset in PRESET_NAMES]
    seed = args.seed if args.seed is not None else int.from_bytes(os.urandom(4), "little")
    letters = [chr(ord("A") + i) for i in range(len(candidates))]
    order = list(range(len(candidates)))
    random.Random(seed).shuffle(order)
    key: dict[str, dict[str, Any]] = {}
    mouth: dict[str, list[dict[str, Any]]] = {}
    fx_cost: dict[str, list[dict[str, Any]]] = {p: [] for p in PRESET_NAMES}
    wet: dict[tuple[str, str], list[bytes]] = {}
    for name, sid in blends:
        argv = warm.worker_argv(engine_dir, sid, args.threads, True, blend_paths.voices)
        worker = warm.Worker(argv, f"bench-{name}")
        worker.start()
        try:
            worker.ready()
            faces.sid = sid
            mouth[name] = []
            for i, line in enumerate(LINES, 1):
                pcm, check = faced_render(worker, line, faces)
                mouth[name].append({"line": i, **check})
                for preset in PRESET_NAMES:
                    processed, took = time_fx(pcm, preset, args.reps)
                    wet.setdefault((name, preset), []).append(processed)
                    secs = _seconds(pcm)
                    fx_cost[preset].append(
                        {"line": i, "audio_s": round(secs, 3), "fx_ms": round(took * 1000, 2),
                         "rtf": round(took / secs, 4), "same_length": True}
                    )  # fmt: skip
        finally:
            worker.kill()
    for slot, letter in zip(order, letters):
        name, sid, preset = candidates[slot]
        parts = wet[(name, preset)]
        for i, pcm in enumerate(parts, 1):
            fx.write_wav(out / f"{letter}-{i}.wav", pcm)
        fx.write_wav(out / f"{letter}.wav", _joined(parts))
        key[letter] = {"voice": name, "sid": sid, "recipe": next(
            v["recipe"] for v in meta["voices"] if v["name"] == name
        ), "fx": preset}  # fmt: skip
    (out / "key.json").write_text(
        json.dumps({"seed": seed, "letters": key, "voices_sha256": meta["output"]["sha256"]},
                   indent=2) + "\n", encoding="utf-8",
    )  # fmt: skip
    metrics = {
        "threads": args.threads,
        "reps": args.reps,
        "lines": list(LINES),
        "synthesis": synthesis,
        "fx": fx_cost,
        "mouth": mouth,
        "letters": letters,
    }
    (out / "metrics.json").write_text(json.dumps(metrics, indent=2) + "\n", encoding="utf-8")
    return metrics


def main(argv: Sequence[str] | None = None) -> int:
    parser = argparse.ArgumentParser(prog="shadowrunr_voice.bench")
    parser.add_argument("--engine-dir", default=os.environ.get("SHADOWRUNR_VOICE_ENGINE_DIR"))
    parser.add_argument("--durations", default=os.environ.get("SHADOWRUNR_VOICE_DURATIONS"))
    parser.add_argument("--recipe", default=str(RECIPE))
    parser.add_argument(
        "--voices-out", default=str(Path.home() / ".shadowrunr" / "voices" / "malakor-voices.bin")
    )
    parser.add_argument("--out", required=True, help="the bench folder")
    parser.add_argument("--threads", type=int, default=4)
    parser.add_argument("--reps", type=int, default=3)
    parser.add_argument("--seed", type=int, help="the letter shuffle (default: random)")
    args = parser.parse_args(argv)
    if not args.engine_dir or not args.durations:
        parser.error("--engine-dir and --durations (or their SHADOWRUNR_VOICE_* env) are needed")
    metrics = run(args)
    # What is printed never names a letter's voice or preset: the key is in key.json alone.
    for name, rows in metrics["synthesis"].items():
        rtfs = ", ".join(f"{r['rtf']:.3f}" for r in rows)
        print(f"synthesis {name}: rtf per line {rtfs}")
    for preset, rows in metrics["fx"].items():
        print(f"fx {preset}: rtf max {max(r['rtf'] for r in rows):.4f}")
    for name, rows in metrics["mouth"].items():
        print(f"mouth {name}: " + ", ".join(
            f"line {r['line']} {r['visemes']} visemes, end {r['timeline_end_ms']}/{r['audio_ms']} ms"
            for r in rows
        ))  # fmt: skip
    print(f"candidates: {', '.join(metrics['letters'])} (key.json holds the mapping)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
