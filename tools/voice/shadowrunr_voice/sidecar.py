# SPDX-License-Identifier: Apache-2.0
"""The sidecar's protocol (contract appendix (voice)): JSON lines on stdio, one object per line.

- Out, first: ``{"type":"hello","protocol":1,"voice","voices","sid","fx","sample_rate",
  "visemes":bool,"plus"?:true,"reason"?}``; ``voices`` is the voices file's name, never its path,
  and ``plus`` says a say's ``delivery`` is heard (every voice that speaks has it).
- In: ``{"type":"say","id","text","delivery"?:{"emotion","intensity","pace"}}`` and
  ``{"type":"cancel","ids":[...]}``.
- Out, per sentence: ``{"type":"audio","id","part","last","text","sample_rate","pcm",
  "visemes"?}``; ``visemes`` is ``[[v, start_ms, end_ms]]`` and is left out when there are none.
- Out, on failure: ``{"type":"error","id","part","reason"}``, and the rest of that line is dropped.
- It exits when its stdin closes, killing any render still running.

**The render pool.** Up to ``--workers`` engine runs render at once, each with ``--num-threads``
set to its share of the cores. Sentences start in arrival order across lines (every sentence of
a line before the next line's first), so line 2's first sentence can render while line 1's last
one does. The output is still strictly ordered: lines in the order they arrived, and each line's
sentences in order, because one sender waits on each sentence in turn, works out its mouth
shapes and sends it. A sentence that finished early waits, rendered, for its turn.
``cancel`` drops the named lines: their waiting sentences never start and their renders in
flight are killed; it answers nothing. A failed sentence ends its line, and that line's later
sentences still rendering are killed.

**The engine.** With ``--engine auto`` (the default) the pool is warm (:mod:`.warm`): one worker
process per slot keeps Kokoro loaded, so a sentence costs its render, not a model load. Without
sherpa_onnx and numpy in this interpreter, or without Kokoro's files, it is the per-sentence
binary (:mod:`.engine`). A warm render cannot be killed: a cancelled one finishes unheard, and
its slot is free when it ends. The warm pool supervises its workers: it starts with those that
load, respawns a dead one in the background, and quarantines one whose loads keep failing.

**The voice.** ``--voices`` names a voices file other than ``kokoro/voices.bin`` (a blend,
:mod:`.blend`); the warm workers, the binary and the durations model all read that one file.
One that is missing, not the stock file's size, or without ``--sid`` among its speakers makes
the voice dead: the hello says why, every ``say`` fails with it, and nothing falls back to
another voice. ``--fx light|heavy`` runs :mod:`.fx` over each sentence after the pause cut and
after its mouth shapes are worked out; the length is unchanged, so the mouth timeline holds. It
needs numpy, and without it the voice is dead too rather than dry.

**Kokoro Plus.** A say with a ``delivery`` is spoken by :mod:`.plus`'s plan: its parts are the
line's sentences (and its clauses at slow pace), each rendered at the plan's speed on the
emotion's blend, with the plan's silence added to its end before the chain, and the chain's
pitch moved by the plan's semitones. Its mouth shapes come from the same sid and speed and end
in ``sil`` over the silence. A say without one is rendered exactly as before.
"""

from __future__ import annotations

import base64
import collections
import dataclasses
import importlib
import json
import sys
import threading
import time
from collections.abc import Callable, Sequence
from dataclasses import dataclass, field
from pathlib import Path
from typing import IO, Any

from .engine import (
    SAMPLE_RATE,
    SPEAKER_BYTES,
    EngineError,
    KokoroPaths,
    Render,
    Rendered,
    paths_for,
    voices_problem,
)
from .plus import (
    PITCH_FLOOR_ST,
    EmotionVoice,
    Plan,
    clamp_note,
    cut,
    edges,
    emotion_voices,
    insert_silence,
    parse_delivery,
    plan_line,
    trim_spans,
)
from .text import split_line
from .visemes import Durations, load_symbols, sentence_visemes, to_ms
from .warm import WarmPool, WarmRender, missing_package, worker_argv

__all__ = [
    "DEFAULT_THREADS",
    "DEFAULT_WORKERS",
    "FX_PRESETS",
    "WARM_THREADS",
    "PROTOCOL",
    "Faces",
    "Out",
    "Settings",
    "Sidecar",
    "hello",
    "open_faces",
    "pitched_chain",
    "plus_reason",
    "voice_problem",
    "wet_chain",
]

PROTOCOL = 1
#: The longest line accepted. Malakor's lines are a few sentences; this bounds a runaway one.
MAX_CHARS = 2000
#: Engine runs at once, and each run's ``--num-threads``, by default: sized by physical cores,
#: ``workers x threads`` at most 6 of an 8-core host's (docs/guide/voice.md).
DEFAULT_WORKERS = 3
DEFAULT_THREADS = 2
#: A warm worker's threads by default. Measured on an 8-core, 16-thread host: 3 workers x 4
#: threads rendered a sentence at RTF 0.32 alone and a 12-sentence burst at 6.8x real time,
#: against 0.41 and 6.0x at 3 x 3 (docs/guide/voice.md). Look-ahead runs below normal priority,
#: so the oversubscription falls on the sentences nobody is waiting for.
WARM_THREADS = 4

#: ``--fx``: the lich chain's presets (:data:`.fx.PRESETS`), or none.
FX_PRESETS = ("none", "light", "heavy")

Send = Callable[[dict[str, Any]], None]
#: One sentence's PCM16 through the FX chain: the same number of samples out.
Wet = Callable[[bytes], bytes]
#: The same, with the chain's pitch moved by some semitones (a line with a delivery).
Pitched = Callable[[bytes, float], bytes]


class Out:
    """The protocol's stream: one compact UTF-8 JSON object per line, flushed, one writer at a
    time."""

    def __init__(self, stream: IO[str]) -> None:
        self._stream = stream
        self._lock = threading.Lock()

    def send(self, message: dict[str, Any]) -> None:
        line = json.dumps(message, separators=(",", ":"), ensure_ascii=False)
        with self._lock:
            self._stream.write(line + "\n")
            self._stream.flush()


@dataclass(frozen=True)
class Settings:
    engine_dir: Path
    sid: int = 3
    voice: str = "af_heart"
    timeout_s: float = 30.0
    durations: Path | None = None
    #: Engine runs at once (the pool).
    workers: int = DEFAULT_WORKERS
    #: Each run's ``--num-threads``; ``None`` is the engine's default (``DEFAULT_THREADS`` for
    #: the binary, ``WARM_THREADS`` for a warm worker).
    threads: int | None = None
    #: Replaces the engine executable: a test's ``[python, fake_engine.py]``.
    command: Sequence[str] | None = None
    #: ``auto`` (warm when it can run), ``warm`` or ``process`` (:mod:`.warm`).
    engine: str = "auto"
    #: Replaces a warm worker's command line: a test's ``[python, fake_warm.py]``.
    warm_command: Sequence[str] | None = None
    #: A voices file other than the engine dir's ``kokoro/voices.bin`` (``--voices``).
    voices: Path | None = None
    #: The FX preset run over each sentence (:data:`FX_PRESETS`).
    fx: str = "none"

    @property
    def paths(self) -> KokoroPaths:
        return paths_for(self.engine_dir, self.voices)


def wet_chain(preset: str) -> Wet | None:
    """The FX chain for ``preset``, or ``None`` for ``none``. Raises ``ImportError`` without
    numpy."""
    if preset == "none":
        return None
    fx = importlib.import_module(".fx", __package__)

    def wet(pcm: bytes) -> bytes:
        return fx.apply(pcm, preset)

    return wet


def pitched_chain(preset: str) -> Pitched | None:
    """The FX chain for ``preset`` with its shift moved by a line's semitones, in the same one
    STFT pass; ``None`` for ``none``. Raises ``ImportError`` without numpy."""
    if preset == "none":
        return None
    fx = importlib.import_module(".fx", __package__)
    base = fx.PRESETS[preset]

    def wet(pcm: bytes, semitones: float) -> bytes:
        return fx.apply(pcm, dataclasses.replace(base, semitones=base.semitones + semitones))

    return wet


def preset_shift(preset: str) -> float | None:
    """The preset's own pitch shift in semitones, or ``None`` with no chain that can run (no
    pitch lever)."""
    if preset not in FX_PRESETS or preset == "none":
        return None
    try:
        fx = importlib.import_module(".fx", __package__)
    except ImportError:
        return None
    return float(fx.PRESETS[preset].semitones)


def own_emotion_voices(settings: Settings) -> dict[str, EmotionVoice]:
    """The voice's emotion blends (:func:`.plus.emotion_voices`); none on the stock file."""
    paths = settings.paths
    if not paths.own_voices:
        return {}
    try:
        speakers = paths.voices.stat().st_size // SPEAKER_BYTES
    except OSError:
        return {}
    return emotion_voices(paths.voices, settings.sid, speakers)


def plus_reason(settings: Settings, voices: dict[str, EmotionVoice]) -> str:
    """The hello's ``plus`` clause: the emotion blends found, and once, the emotions whose pitch
    is held at the floor."""
    count = len(voices)
    notes = [f"{count} emotion voice{'' if count == 1 else 's'}" if voices else "no emotion voices"]
    shift = preset_shift(settings.fx)
    if shift is None:
        notes.append("no pitch lever without fx")
    else:
        held = clamp_note(voices, shift)
        if held:
            notes.append(f"pitch held at {PITCH_FLOOR_ST:.1f} st for {', '.join(held)}")
    return f"plus ({'; '.join(notes)})"


class Faces:
    """The warm durations model and the token table: one sentence's mouth shapes at a time."""

    def __init__(self, durations: Durations, symbols: dict[int, str], sid: int) -> None:
        self.durations = durations
        self.symbols = symbols
        self.sid = sid

    def visemes(
        self,
        rendered: Rendered,
        sample_rate: int,
        *,
        sid: int | None = None,
        speed: float = 1.0,
        keep: tuple[int, int] | None = None,
        pad: int = 0,
    ) -> list[list[Any]] | None:
        """The sentence's ``[[v, start_ms, end_ms]]``, or ``None``: the face is never worth the
        sentence, so any failure here sends the audio without it. ``sid`` and ``speed`` are the
        render's own (a line with a delivery); only samples ``keep`` of the audio are sent, and
        ``pad`` samples of silence follow them."""
        if not rendered.tokens or not rendered.time_map:
            return None
        try:
            spans = sentence_visemes(
                self.durations,
                self.symbols,
                batches=rendered.tokens,
                raw_samples=rendered.raw_samples,
                time_map=rendered.time_map,
                pcm=rendered.pcm,
                sample_rate=sample_rate,
                sid=self.sid if sid is None else sid,
                speed=speed,
            )
        except Exception as exc:
            print(f"voice: no visemes for a sentence: {exc!r}", file=sys.stderr, flush=True)
            return None
        samples = len(rendered.pcm) // 2
        if keep is not None and keep != (0, samples):
            spans = trim_spans(spans, *keep)
            samples = keep[1] - keep[0]
        if pad > 0:
            spans = insert_silence(spans, samples, pad)
            samples += pad
        return to_ms(spans, samples, sample_rate)


def open_faces(settings: Settings, paths: KokoroPaths) -> tuple[Faces | None, str | None]:
    """The warm durations model, or ``None`` and why the voice has no mouth shapes."""
    if settings.durations is None:
        return None, "no mouth shapes: no durations model was given"
    if not settings.durations.is_file():
        return None, "no mouth shapes: the durations model is missing"
    try:
        import numpy  # noqa: F401
        import onnxruntime  # noqa: F401
    except ImportError as exc:
        return None, f"no mouth shapes: {exc.name or 'numpy/onnxruntime'} is not installed"
    try:
        faces = Faces(
            Durations(settings.durations, paths.voices), load_symbols(paths.tokens), settings.sid
        )
        faces.durations.frames([0, 16, 0], settings.sid)  # warm: the first real run is not slow
    except Exception as exc:
        print(f"voice: the durations model would not load: {exc!r}", file=sys.stderr, flush=True)
        return None, f"no mouth shapes: the durations model would not load ({type(exc).__name__})"
    return faces, None


def open_warm(
    settings: Settings, paths: KokoroPaths, faced: bool
) -> tuple[WarmPool | None, str | None]:
    """The warm pool, loaded, or ``None`` and why not (``None`` when it was not wanted).

    ``auto`` takes the warm pool when this interpreter has sherpa_onnx and numpy and Kokoro's
    files are there, and otherwise falls back to the per-sentence binary. A test's fake binary
    (``command``) is never paired with the real warm engine."""
    if settings.engine == "process" or (
        settings.engine == "auto" and settings.command is not None and not settings.warm_command
    ):
        return None, None
    if settings.warm_command is None:
        lacking = missing_package()
        if lacking is not None:
            return None, f"{lacking} is not installed"
        if paths.missing(exe=False):
            return None, "Kokoro's files are missing"
        threads = settings.threads if settings.threads is not None else WARM_THREADS
        argv = worker_argv(settings.engine_dir, settings.sid, threads, faced, settings.voices)
    else:
        argv = [*settings.warm_command, *(["--faced"] if faced else [])]
    pool = WarmPool(argv, settings.workers)
    try:
        load_ms = pool.start()
    except EngineError as exc:
        print(f"voice: the warm engine would not load: {exc}", file=sys.stderr, flush=True)
        return None, f"it would not load: {exc}"
    print(
        f"voice: warm engine, {pool.loaded} of {len(pool.workers)} workers loaded in {load_ms} ms",
        file=sys.stderr,
        flush=True,
    )
    return pool, None


def voice_problem(settings: Settings) -> str | None:
    """Why this voice cannot speak at all, before any engine is loaded: its voices file, or an
    FX chain this interpreter cannot run. ``None`` when neither is in the way."""
    paths = settings.paths
    if paths.own_voices:
        problem = voices_problem(paths, settings.sid)
        if problem is not None:
            return problem
    if settings.fx not in FX_PRESETS:
        return f"fx {settings.fx} is not a preset ({', '.join(FX_PRESETS)})"
    try:
        wet_chain(settings.fx)
    except ImportError as exc:
        return f"fx {settings.fx} needs {exc.name or 'numpy'}, which is not installed"
    return None


def hello(
    settings: Settings,
) -> tuple[dict[str, Any], str | None, Faces | None, WarmPool | None]:
    """The hello, the reason every ``say`` fails with (``None`` when the engine is there), the
    face when the visemes are on, and the warm pool when it is the engine.

    ``reason`` names what the voice lacks (mouth shapes), then the engine in use: ``engine:
    warm (N workers loaded in S s)`` (``N of M workers`` when some would not load and are being
    respawned) or ``engine: process`` with why the warm one is not. The
    hello has no field of its own for it. A dead voice's reason names no engine: it says why."""
    paths = settings.paths
    message: dict[str, Any] = {
        "type": "hello",
        "protocol": PROTOCOL,
        "voice": settings.voice,
        "voices": paths.voices.name,
        "sid": settings.sid,
        "fx": settings.fx,
        "sample_rate": SAMPLE_RATE,
        "visemes": False,
    }
    dead = voice_problem(settings)
    if dead is not None:
        message["reason"] = dead
        return message, dead, None, None
    faces, why = None, None
    if not paths.missing(exe=False):
        faces, why = open_faces(settings, paths)
    warm, not_warm = open_warm(settings, paths, faces is not None)
    if warm is None and settings.engine == "warm":
        dead = f"the warm engine is not running: {not_warm}"
        message["reason"] = dead
        return message, dead, None, None
    missing = paths.missing(exe=warm is None and settings.command is None)
    if missing:
        if warm is not None:
            warm.close()
        more = f" (and {len(missing) - 1} more)" if len(missing) > 1 else ""
        dead = f"the engine is not installed: {missing[0]} is missing from the engine dir{more}"
        message["reason"] = dead
        return message, dead, None, None
    if warm is not None:
        took = f"{warm.load_ms / 1000:.1f} s"
        if warm.loaded < len(warm.workers):
            engine = f"engine: warm ({warm.loaded} of {len(warm.workers)} workers loaded in {took})"
        else:
            engine = f"engine: warm ({len(warm.workers)} workers loaded in {took})"
    elif not_warm is not None:
        engine = f"engine: process (the warm engine is off: {not_warm})"
    else:
        engine = "engine: process"
    if faces is not None:
        message["visemes"] = True
    message["plus"] = True
    plus = plus_reason(settings, own_emotion_voices(settings))
    message["reason"] = f"{why}; {engine}; {plus}" if why is not None else f"{engine}; {plus}"
    return message, None, faces, warm


@dataclass
class _Line:
    id: Any
    text: str
    at: float
    #: The sentences, or empty when the line fails as a whole (``problem``).
    parts: list[str] = field(default_factory=list)
    #: Why the line is an error at part 0 without a render: no engine, too long, no words.
    problem: str | None = None
    #: The renders started so far, in part order: ``renders[i]`` is ``parts[i]``.
    renders: list[Render | WarmRender] = field(default_factory=list)
    #: Parts already sent; ``parts[sent]`` is the one the sender waits for next.
    sent: int = 0
    cancelled: bool = False
    #: How a line with a delivery is spoken (:mod:`.plus`); ``None`` is as before.
    plan: Plan | None = None


class Sidecar:
    """Lines in, sentences out. ``feed`` takes each message from the reader.

    A pool of up to ``workers`` engine runs renders sentences in arrival order, across lines; one
    sender thread sends them, waiting on each in turn, so the output stays in order however the
    renders finish."""

    def __init__(
        self,
        settings: Settings,
        send: Send,
        *,
        dead: str | None = None,
        faces: Faces | None = None,
        warm: WarmPool | None = None,
        wet: Wet | None = None,
        pitched: Pitched | None = None,
    ) -> None:
        self.settings = settings
        self.paths = settings.paths
        self.send = send
        self.dead = dead
        self.faces = faces
        self.warm = warm
        #: The FX chain; ``wet`` replaces it (a test's). A chain that cannot run is the hello's
        #: ``dead`` reason, so it is built only for a voice that speaks.
        self.wet = wet
        if wet is None and dead is None:
            self.wet = wet_chain(settings.fx)
        #: The chain with a line's pitch (``pitched`` replaces it), and Kokoro Plus's inputs:
        #: the preset's own shift and the voice's emotion blends.
        self.pitched = pitched
        self.preset_st: float | None = None
        self.emotion_voices: dict[str, EmotionVoice] = {}
        if dead is None:
            if pitched is None and wet is None:
                self.pitched = pitched_chain(settings.fx)
            if self.pitched is not None:
                self.preset_st = preset_shift(settings.fx)
            self.emotion_voices = own_emotion_voices(settings)
        self.workers = len(warm.workers) if warm is not None else max(settings.workers, 1)
        self.threads = max(settings.threads if settings.threads is not None else DEFAULT_THREADS, 1)
        self._lock = threading.Condition()
        #: Lines not yet fully sent, in arrival order; the first is the one being sent.
        self._lines: collections.deque[_Line] = collections.deque()
        #: Engine runs started and not yet ended.
        self._running = 0
        self._closed = False
        self._worker = threading.Thread(target=self._work, name="voice-sender", daemon=True)
        self._worker.start()

    # -- in --------------------------------------------------------------------------------------

    def feed(self, raw: str | bytes) -> None:
        try:
            message = json.loads(raw)
        except ValueError:
            print("voice: a line that is not JSON was ignored", file=sys.stderr, flush=True)
            return
        if not isinstance(message, dict):
            return
        kind = message.get("type")
        if kind == "say":
            self._say(message)
        elif kind == "cancel":
            ids = message.get("ids")
            if isinstance(ids, list):
                self.cancel(ids)

    def _say(self, message: dict[str, Any]) -> None:
        ident, text = message.get("id"), message.get("text")
        if not _is_id(ident):
            print("voice: a say with no usable id was ignored", file=sys.stderr, flush=True)
            return
        if not isinstance(text, str):
            self.send(_error(ident, 0, "the say has no text"))
            return
        line = _Line(ident, text, time.monotonic())
        delivery = parse_delivery(message.get("delivery"))
        if self.dead is not None:
            line.problem = self.dead
        elif len(text) > MAX_CHARS:
            line.problem = f"the line is longer than {MAX_CHARS} characters"
        elif delivery is None:
            line.parts = split_line(text)
        else:
            line.plan = plan_line(
                text,
                delivery,
                base_sid=self.settings.sid,
                voices=self.emotion_voices,
                preset_st=self.preset_st,
            )
            line.parts = line.plan.parts
            print(f"voice: plus {line.plan.describe()}", file=sys.stderr, flush=True)
        if line.problem is None and not line.parts:
            line.problem = "the line has no words"
        with self._lock:
            if self._closed:
                return
            self._lines.append(line)
            self._pump()
            self._lock.notify_all()

    def cancel(self, ids: list[Any]) -> None:
        """Drop the named lines: their waiting sentences are never started, and their renders
        in flight are killed, or on the warm engine finish unheard. Nothing more is sent for
        them."""
        wanted = [i for i in ids if _is_id(i)]
        renders: list[Render | WarmRender] = []
        with self._lock:
            kept: collections.deque[_Line] = collections.deque()
            for line in self._lines:
                if _named(line.id, wanted):
                    line.cancelled = True
                    renders += line.renders
                else:
                    kept.append(line)
            self._lines = kept
            self._lock.notify_all()
        for render in renders:
            render.cancel()

    def close(self) -> None:
        """stdin closed: nothing more is rendered, and every render in flight is killed."""
        with self._lock:
            self._closed = True
            renders = [r for line in self._lines for r in line.renders]
            for line in self._lines:
                line.cancelled = True
            self._lines.clear()
            self._lock.notify_all()
        for render in renders:
            render.cancel()
        self._worker.join(timeout=5)
        if self.warm is not None:
            self.warm.close()

    # -- the pool --------------------------------------------------------------------------------

    def _pump(self) -> None:
        """Start sentences, the earliest unstarted first, while the pool has room. The lock is
        held.

        **Head of line first.** The sentence the sender waits for next is always the earliest
        unstarted one, so it takes the first free worker: a later sentence only ever gets a
        worker nothing earlier needs. It also runs at full priority, while a look-ahead run
        starts below normal (Windows) and is promoted once it is the one waited for, so
        look-ahead never takes CPU from the sentence the viewer is waiting to hear."""
        if self._closed:
            return
        for line in self._lines:
            if self._running >= self.workers:
                return
            while not line.cancelled and len(line.renders) < len(line.parts):
                if self._running >= self.workers:
                    return
                urgent = line is self._lines[0] and len(line.renders) == line.sent
                sentence = line.parts[len(line.renders)]
                plan = line.plan
                render: Render | WarmRender
                if self.warm is not None:
                    render = WarmRender(
                        self.warm,
                        sentence,
                        self.settings.timeout_s,
                        faced=self.faces is not None,
                        urgent=urgent,
                        on_done=self._ended,
                        sid=plan.sid if plan is not None else None,
                        speed=plan.speed if plan is not None else None,
                    )
                else:
                    render = Render(
                        self.paths,
                        plan.sid if plan is not None else self.settings.sid,
                        sentence,
                        self.settings.timeout_s,
                        faced=self.faces is not None,
                        command=self.settings.command,
                        threads=self.threads,
                        urgent=urgent,
                        on_done=self._ended,
                        speed=plan.speed if plan is not None else None,
                    )
                line.renders.append(render)
                self._running += 1
                render.start()

    def _ended(self, _render: Render | WarmRender) -> None:
        """A render's thread: its process is gone, or its warm worker is free, so its slot is."""
        with self._lock:
            self._running -= 1
            self._pump()
            self._lock.notify_all()

    # -- the sender ------------------------------------------------------------------------------

    def _work(self) -> None:
        while True:
            with self._lock:
                while not self._lines and not self._closed:
                    self._lock.wait()
                if self._closed:
                    return
                line = self._lines[0]
            try:
                self._speak(line)
            except Exception as exc:  # one line's bug must not end the voice
                print(f"voice: a line failed: {exc!r}", file=sys.stderr, flush=True)
                self.send(_error(line.id, 0, f"the sidecar failed ({type(exc).__name__})"))
            finally:
                with self._lock:
                    if self._lines and self._lines[0] is line:
                        self._lines.popleft()
                    left = list(line.renders)
                    self._pump()
                # A failed line's later sentences may still be running: they are not wanted.
                for render in left:
                    render.cancel()

    def _plus(
        self, plan: Plan, part: int, rendered: Rendered
    ) -> tuple[list[list[Any]] | None, bytes]:
        """A part of a line with a delivery: Kokoro's quiet cut at an edge that borders a pause,
        its own pause added to its end, its mouth shapes from its own sid and speed with ``sil``
        over the pause, then the chain with its pitch. The pause goes in before the chain, so
        the crypt rings into it."""
        pad = round(plan.pauses_ms[part] * SAMPLE_RATE / 1000)
        lead = part > 0 and plan.pauses_ms[part - 1] > 0
        a, b = edges(rendered.pcm, SAMPLE_RATE, lead=lead, tail=pad > 0)
        visemes = None
        if self.faces is not None:
            visemes = self.faces.visemes(
                rendered, SAMPLE_RATE, sid=plan.sid, speed=plan.speed, keep=(a, b), pad=pad
            )
        dry = (
            rendered.pcm
            if (a, b) == (0, len(rendered.pcm) // 2)
            else cut(rendered.pcm, a, b, SAMPLE_RATE)
        )
        pcm = dry + bytes(2 * pad)
        if plan.pitch_st and self.pitched is not None:
            return visemes, self.pitched(pcm, plan.pitch_st)
        return visemes, self.wet(pcm) if self.wet is not None else pcm

    def _speak(self, line: _Line) -> None:
        if line.problem is not None:
            self.send(_error(line.id, 0, line.problem))
            return
        for part, sentence in enumerate(line.parts):
            with self._lock:
                while len(line.renders) <= part and not line.cancelled and not self._closed:
                    self._lock.wait()
                if line.cancelled or self._closed:
                    return
                render = line.renders[part]
            render.promote()
            try:
                rendered = render.wait()
            except EngineError as exc:
                self.send(_error(line.id, part, str(exc)))
                return
            if rendered is None:
                return
            plan = line.plan
            if plan is None:
                visemes = self.faces.visemes(rendered, SAMPLE_RATE) if self.faces else None
                # The chain keeps the length, so mouth shapes worked out on the dry audio hold.
                pcm = self.wet(rendered.pcm) if self.wet is not None else rendered.pcm
            else:
                visemes, pcm = self._plus(plan, part, rendered)
            with self._lock:
                if line.cancelled or self._closed:
                    return
            message: dict[str, Any] = {
                "type": "audio",
                "id": line.id,
                "part": part,
                "last": part == len(line.parts) - 1,
                "text": sentence,
                "sample_rate": SAMPLE_RATE,
                "pcm": base64.b64encode(pcm).decode("ascii"),
            }
            if visemes is not None:
                message["visemes"] = visemes
            self.send(message)
            with self._lock:
                line.sent = part + 1
            print(
                f"voice: part {part} rendered in {render.took_ms} ms, sent "
                f"{round((time.monotonic() - line.at) * 1000)} ms after its say",
                file=sys.stderr,
                flush=True,
            )


def _is_id(value: Any) -> bool:
    return (isinstance(value, str) and bool(value)) or (
        isinstance(value, int) and not isinstance(value, bool)
    )


def _named(ident: Any, ids: list[Any]) -> bool:
    return any(type(ident) is type(i) and ident == i for i in ids)


def _error(ident: Any, part: int, reason: str) -> dict[str, Any]:
    return {"type": "error", "id": ident, "part": part, "reason": reason}
