# SPDX-License-Identifier: Apache-2.0
"""The sidecar's protocol (contract appendix (voice)): JSON lines on stdio, one object per line.

- Out, first: ``{"type":"hello","protocol":1,"voice","sample_rate","visemes":bool,"reason"?}``.
- In: ``{"type":"say","id","text"}`` and ``{"type":"cancel","ids":[...]}``.
- Out, per sentence: ``{"type":"audio","id","part","last","text","sample_rate","pcm",
  "visemes"?}``; ``visemes`` is ``[[v, start_ms, end_ms]]`` and is left out when there are none.
- Out, on failure: ``{"type":"error","id","part","reason"}``, and the rest of that line is dropped.
- It exits when its stdin closes, killing any render still running.

Lines are rendered one at a time, in the order they arrive. Within a line the next sentence's
engine run starts as soon as the current one's audio is in, before the current sentence's mouth
shapes are worked out and it is sent, so the engine is never idle while a line has more to say.
``cancel`` drops the named lines still waiting and kills the one being rendered; it answers
nothing.
"""

from __future__ import annotations

import base64
import collections
import json
import sys
import threading
import time
from collections.abc import Callable, Sequence
from dataclasses import dataclass
from pathlib import Path
from typing import IO, Any

from .engine import SAMPLE_RATE, EngineError, KokoroPaths, Render, Rendered, paths_for
from .text import split_line
from .visemes import Durations, load_symbols, sentence_visemes, to_ms

__all__ = ["PROTOCOL", "Faces", "Out", "Sidecar", "Settings", "hello", "open_faces"]

PROTOCOL = 1
#: The longest line accepted. Malakor's lines are a few sentences; this bounds a runaway one.
MAX_CHARS = 2000

Send = Callable[[dict[str, Any]], None]


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
    #: Replaces the engine executable: a test's ``[python, fake_engine.py]``.
    command: Sequence[str] | None = None


class Faces:
    """The warm durations model and the token table: one sentence's mouth shapes at a time."""

    def __init__(self, durations: Durations, symbols: dict[int, str], sid: int) -> None:
        self.durations = durations
        self.symbols = symbols
        self.sid = sid

    def visemes(self, rendered: Rendered, sample_rate: int) -> list[list[Any]] | None:
        """The sentence's ``[[v, start_ms, end_ms]]``, or ``None``: the face is never worth the
        sentence, so any failure here sends the audio without it."""
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
                sid=self.sid,
            )
        except Exception as exc:
            print(f"voice: no visemes for a sentence: {exc!r}", file=sys.stderr, flush=True)
            return None
        return to_ms(spans, len(rendered.pcm) // 2, sample_rate)


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


def hello(settings: Settings) -> tuple[dict[str, Any], str | None, Faces | None]:
    """The hello, the reason every ``say`` fails with (``None`` when the engine is there), and
    the face when the visemes are on."""
    paths = paths_for(settings.engine_dir)
    message: dict[str, Any] = {
        "type": "hello",
        "protocol": PROTOCOL,
        "voice": settings.voice,
        "sample_rate": SAMPLE_RATE,
        "visemes": False,
    }
    missing = paths.missing(exe=settings.command is None)
    if missing:
        more = f" (and {len(missing) - 1} more)" if len(missing) > 1 else ""
        dead = f"the engine is not installed: {missing[0]} is missing from the engine dir{more}"
        message["reason"] = dead
        return message, dead, None
    faces, why = open_faces(settings, paths)
    if faces is not None:
        message["visemes"] = True
    elif why is not None:
        message["reason"] = why
    return message, None, faces


@dataclass
class _Line:
    id: Any
    text: str
    at: float


class Sidecar:
    """Lines in, sentences out. ``feed`` takes each message from the reader; one worker thread
    renders."""

    def __init__(
        self,
        settings: Settings,
        send: Send,
        *,
        dead: str | None = None,
        faces: Faces | None = None,
    ) -> None:
        self.settings = settings
        self.paths = paths_for(settings.engine_dir)
        self.send = send
        self.dead = dead
        self.faces = faces
        self._lock = threading.Condition()
        self._queue: collections.deque[_Line] = collections.deque()
        self._current: _Line | None = None
        self._renders: list[Render] = []
        self._cancelled = False
        self._closed = False
        self._worker = threading.Thread(target=self._work, name="voice-worker", daemon=True)
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
        with self._lock:
            self._queue.append(_Line(ident, text, time.monotonic()))
            self._lock.notify()

    def cancel(self, ids: list[Any]) -> None:
        wanted = [i for i in ids if _is_id(i)]
        with self._lock:
            kept = [line for line in self._queue if not _named(line.id, wanted)]
            self._queue = collections.deque(kept)
            current = self._current
            if current is None or not _named(current.id, wanted):
                return
            self._cancelled = True
            renders = list(self._renders)
        for render in renders:
            render.cancel()

    def close(self) -> None:
        """stdin closed: nothing more is rendered, and the render in flight is killed."""
        with self._lock:
            self._closed = True
            self._queue.clear()
            self._cancelled = True
            renders = list(self._renders)
            self._lock.notify()
        for render in renders:
            render.cancel()
        self._worker.join(timeout=5)

    # -- the worker ------------------------------------------------------------------------------

    def _work(self) -> None:
        while True:
            with self._lock:
                while not self._queue and not self._closed:
                    self._lock.wait()
                if self._closed:
                    return
                line = self._queue.popleft()
                self._current, self._cancelled, self._renders = line, False, []
            try:
                self._speak(line)
            except Exception as exc:  # one line's bug must not end the voice
                print(f"voice: a line failed: {exc!r}", file=sys.stderr, flush=True)
                self.send(_error(line.id, 0, f"the sidecar failed ({type(exc).__name__})"))
            finally:
                with self._lock:
                    self._current, self._renders = None, []

    def _start(self, sentence: str) -> Render | None:
        render = Render(
            self.paths,
            self.settings.sid,
            sentence,
            self.settings.timeout_s,
            faced=self.faces is not None,
            command=self.settings.command,
        )
        with self._lock:
            if self._cancelled:
                return None
            self._renders.append(render)
        return render.start()

    def _speak(self, line: _Line) -> None:
        if self.dead is not None:
            self.send(_error(line.id, 0, self.dead))
            return
        if len(line.text) > MAX_CHARS:
            self.send(_error(line.id, 0, f"the line is longer than {MAX_CHARS} characters"))
            return
        parts = split_line(line.text)
        if not parts:
            self.send(_error(line.id, 0, "the line has no words"))
            return
        pending = self._start(parts[0])
        for part, sentence in enumerate(parts):
            if pending is None:
                return
            render = pending
            try:
                rendered = render.wait()
            except EngineError as exc:
                self.send(_error(line.id, part, str(exc)))
                return
            if rendered is None:
                return
            pending = self._start(parts[part + 1]) if part + 1 < len(parts) else None
            visemes = self.faces.visemes(rendered, SAMPLE_RATE) if self.faces else None
            with self._lock:
                if self._cancelled:
                    if pending is not None:
                        pending.cancel()
                    return
            message: dict[str, Any] = {
                "type": "audio",
                "id": line.id,
                "part": part,
                "last": part == len(parts) - 1,
                "text": sentence,
                "sample_rate": SAMPLE_RATE,
                "pcm": base64.b64encode(rendered.pcm).decode("ascii"),
            }
            if visemes is not None:
                message["visemes"] = visemes
            self.send(message)
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
