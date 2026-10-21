# SPDX-License-Identifier: Apache-2.0
"""Kokoro kept loaded: one warm worker process per pool slot, each with its own
``sherpa_onnx.OfflineTts``, so a sentence costs its render and not a model load.

**Why a process per worker, not a thread.** ``generate`` releases the GIL, so threads would run
in parallel, but the mouth shapes need each sentence's token ids, and sherpa prints them only to
the native stderr (``debug``), which one process shares between all its threads. A worker of its
own gives every render an unshared stream, and a process whose priority class can drop below
normal for a look-ahead sentence and rise once it is the one waited for, as the binary's runs do.

**The worker** (``python -m shadowrunr_voice.warm``) loads the model, renders one short sentence
to warm it, prints ``@@warm {"ready":true,"load_ms":N}``, then reads JSON lines on stdin:
``{"text","urgent"}`` is a sentence, answered ``@@warm {"ok":true,"pcm":<base64 PCM16>}`` or
``@@warm {"ok":false,"reason"}``, and ``{"urgent"}`` alone changes the priority of the sentence
rendering now. The worker sets its own priority: what the pool spawned may be a venv's launcher
rather than the process that renders. Its stdout and stderr are one pipe: sherpa's native writes are
unbuffered and done before ``generate`` returns, so a sentence's token line always comes before
its answer. The audio is the engine's own: sherpa's pause cut when the visemes are off, and the
uncut audio when they are on, which :func:`.engine.scale_silence` then cuts exactly as the
binary does. It exits when its stdin closes.

**A render cannot be stopped mid-sentence.** A cancelled sentence's result is thrown away and
its worker returns to the pool when it finishes; a sentence past its timeout kills the worker,
and the next sentence that needs it starts a new one.
"""

from __future__ import annotations

import argparse
import base64
import importlib.util
import json
import queue
import subprocess
import sys
import threading
import time
from collections import deque
from collections.abc import Callable, Sequence
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any

from .engine import (
    BELOW_NORMAL_PRIORITY_CLASS,
    NORMAL_PRIORITY_CLASS,
    SAMPLE_RATE,
    SILENCE_SCALE,
    EngineError,
    KokoroPaths,
    Rendered,
    creation_flags,
    paths_for,
    scale_silence,
    token_lines,
)

__all__ = [
    "ENGINES",
    "MARK",
    "WarmPool",
    "WarmRender",
    "Worker",
    "missing_package",
    "worker_argv",
]

#: ``--engine``: the warm pool, the per-sentence binary, or the warm pool when it can run.
ENGINES = ("auto", "warm", "process")
#: Starts each line the worker writes for the pool; anything else is sherpa's or Python's.
MARK = b"@@warm "
#: From a worker's start to its ready line: the model load and one warm-up sentence.
LOAD_TIMEOUT_S = 90.0
#: Lines of a worker's other output kept for the log when it fails.
TAIL = 6
#: The sentence a worker renders before it is ready, so the first real one is not slow.
WARM_UP = "Ready."
#: The package's parent directory: a worker imports ``shadowrunr_voice`` from there.
TOOLS_DIR = Path(__file__).resolve().parent.parent


def missing_package() -> str | None:
    """The package the warm engine needs and this interpreter lacks, or ``None``. Nothing is
    imported: this is the cheap check before workers are started."""
    for name in ("sherpa_onnx", "numpy"):
        try:
            if importlib.util.find_spec(name) is None:
                return name
        except (ImportError, ValueError):
            return name
    return None


def worker_argv(engine_dir: Path, sid: int, threads: int, faced: bool) -> list[str]:
    """A worker's command line: this interpreter, the package's module, its settings."""
    argv = [
        sys.executable,
        "-m",
        "shadowrunr_voice.warm",
        "--engine-dir",
        str(engine_dir),
        "--sid",
        str(sid),
        "--threads",
        str(max(threads, 1)),
    ]
    if faced:
        argv.append("--faced")
    return argv


class Worker:
    """One warm worker process: its pipe read on a thread of its own, one sentence at a time."""

    def __init__(self, argv: Sequence[str], name: str) -> None:
        self.argv = list(argv)
        self.name = name
        self.proc: subprocess.Popen[bytes] | None = None
        self._lines: queue.Queue[bytes | None] = queue.Queue()
        self._tail: deque[str] = deque(maxlen=TAIL)
        self._stdin = threading.Lock()
        self.urgent = True
        #: The last load's milliseconds, from the worker's ready line.
        self.load_ms = 0
        #: Set once this side has killed the worker: its end is then no news.
        self.killed = False

    @property
    def alive(self) -> bool:
        return self.proc is not None and self.proc.poll() is None

    def start(self) -> None:
        """Spawn the worker; :meth:`ready` waits for its model."""
        self._lines = queue.Queue()
        self.urgent = True
        self.killed = False
        try:
            self.proc = subprocess.Popen(
                self.argv,
                cwd=TOOLS_DIR,
                stdin=subprocess.PIPE,
                stdout=subprocess.PIPE,
                stderr=subprocess.STDOUT,
                creationflags=creation_flags(True),
            )
        except OSError as exc:
            self.proc = None
            raise EngineError(f"the warm engine would not start ({type(exc).__name__})") from None
        threading.Thread(
            target=self._read, args=(self.proc, self._lines), name=f"voice-{self.name}", daemon=True
        ).start()

    def ready(self, timeout_s: float = LOAD_TIMEOUT_S) -> int:
        """Wait for the ready line; its load time in milliseconds. Raises :class:`EngineError`."""
        reply = self._reply(timeout_s, [], what="load")
        if not reply.get("ready"):
            self.kill()
            raise EngineError(str(reply.get("reason") or "the warm engine did not load"))
        self.load_ms = int(reply.get("load_ms") or 0)
        return self.load_ms

    def render(
        self, text: str, timeout_s: float, urgent: bool = True
    ) -> tuple[bytes, list[list[int]]]:
        """One sentence: its PCM16 and the token lines printed for it. ``urgent`` false runs it
        below normal priority until :meth:`set_urgent` raises it."""
        if self.proc is None or not self.alive:
            raise EngineError("the warm engine is not running")
        self._drain()
        with self._stdin:
            self.urgent = urgent
            sent = self._write({"text": text, "urgent": urgent})
        if not sent:
            self._died()
            raise EngineError("the warm engine stopped")
        printed: list[str] = []
        reply = self._reply(timeout_s, printed, what="render")
        if not reply.get("ok"):
            raise EngineError(str(reply.get("reason") or "the warm engine failed"))
        try:
            pcm = base64.b64decode(str(reply.get("pcm") or ""), validate=True)
        except ValueError:
            raise EngineError("the warm engine's audio was not base64") from None
        return pcm, token_lines("\n".join(printed))

    def set_urgent(self, urgent: bool) -> None:
        """Full priority for the sentence being waited for, below normal for a look-ahead. The
        worker sets its own: the process spawned may be a venv's launcher, not the one that
        renders."""
        with self._stdin:
            if self.urgent == urgent or not self.alive:
                return
            self.urgent = urgent
            self._write({"urgent": urgent})

    def _write(self, message: dict[str, Any]) -> bool:
        """One line to the worker; the caller holds ``_stdin``."""
        proc = self.proc
        if proc is None or proc.stdin is None:
            return False
        try:
            proc.stdin.write(json.dumps(message).encode("ascii") + b"\n")
            proc.stdin.flush()
        except (OSError, ValueError):
            return False
        return True

    def kill(self) -> None:
        proc = self.proc
        if proc is None:
            return
        self.killed = True
        if proc.poll() is None:
            proc.kill()
        try:
            proc.wait(timeout=5)
        except subprocess.TimeoutExpired:
            pass
        for stream in (proc.stdin, proc.stdout):
            if stream is not None:
                try:
                    stream.close()
                except OSError:
                    pass

    # -- the pipe --------------------------------------------------------------------------------

    @staticmethod
    def _read(proc: subprocess.Popen[bytes], lines: queue.Queue[bytes | None]) -> None:
        assert proc.stdout is not None
        try:
            for raw in proc.stdout:
                lines.put(raw)
        except (OSError, ValueError):
            pass
        lines.put(None)

    def _drain(self) -> None:
        """Anything printed between sentences belongs to none of them."""
        while True:
            try:
                raw = self._lines.get_nowait()
            except queue.Empty:
                return
            if raw is None:
                self._lines.put(None)
                return
            self._keep(raw)

    def _keep(self, raw: bytes) -> str:
        line = raw.decode("utf-8", "replace").rstrip("\r\n")
        if line.strip():
            self._tail.append(" ".join(line.split())[:200])
        return line

    def _reply(self, timeout_s: float, printed: list[str], *, what: str) -> dict[str, Any]:
        deadline = time.monotonic() + timeout_s
        while True:
            left = deadline - time.monotonic()
            try:
                if left <= 0:
                    raise queue.Empty
                raw = self._lines.get(timeout=left)
            except queue.Empty:
                self.kill()
                raise EngineError(
                    f"the warm engine's {what} timed out after {round(timeout_s * 1000)} ms"
                ) from None
            if raw is None:
                self._died()
                raise EngineError("the warm engine stopped")
            at = raw.find(MARK)
            if at >= 0:
                if at > 0:
                    printed.append(self._keep(raw[:at]))
                try:
                    reply = json.loads(raw[at + len(MARK) :])
                except ValueError:
                    continue
                if isinstance(reply, dict):
                    return reply
                continue
            printed.append(self._keep(raw))

    def _died(self) -> None:
        """The worker is gone: its last words go to the operator's log, never into a reason."""
        proc = self.proc
        if proc is None or self.killed:
            return
        try:
            code: int | None = proc.wait(timeout=2)
        except subprocess.TimeoutExpired:
            code = None
        tail = " | ".join(self._tail)
        print(f"voice: warm {self.name} exited ({code}): {tail}", file=sys.stderr, flush=True)
        self.kill()


class WarmPool:
    """The workers, each lent to one render at a time."""

    def __init__(self, argv: Sequence[str], workers: int) -> None:
        self.workers = [Worker(argv, f"worker-{i}") for i in range(max(workers, 1))]
        self._idle: list[Worker] = list(self.workers)
        self._cond = threading.Condition()
        self._closed = False
        #: Wall time from the first spawn to the last ready line.
        self.load_ms = 0

    def start(self, timeout_s: float = LOAD_TIMEOUT_S) -> int:
        """Every worker loaded, together; the wall time it took. Raises :class:`EngineError`
        (and stops them all) when one does not load."""
        began = time.monotonic()
        try:
            for worker in self.workers:
                worker.start()
            for worker in self.workers:
                worker.ready(max(timeout_s - (time.monotonic() - began), 0.1))
        except EngineError:
            self.close()
            raise
        self.load_ms = round((time.monotonic() - began) * 1000)
        return self.load_ms

    def acquire(self) -> Worker:
        """An idle worker, a running one first; a stopped one is started again here."""
        with self._cond:
            while not self._idle and not self._closed:
                self._cond.wait()
            if self._closed:
                raise EngineError("the voice is stopping")
            live = [w for w in self._idle if w.alive]
            worker = live[0] if live else self._idle[0]
            self._idle.remove(worker)
        if not worker.alive:
            try:
                worker.kill()
                worker.start()
                worker.ready()
                print(
                    f"voice: warm {worker.name} restarted in {worker.load_ms} ms",
                    file=sys.stderr,
                    flush=True,
                )
            except EngineError:
                self.release(worker)
                raise
        return worker

    def release(self, worker: Worker) -> None:
        with self._cond:
            self._idle.append(worker)
            self._cond.notify_all()

    def close(self) -> None:
        with self._cond:
            self._closed = True
            self._cond.notify_all()
        for worker in self.workers:
            worker.kill()


@dataclass
class WarmRender:
    """One sentence on a warm worker, with :class:`.engine.Render`'s face: ``start``, ``wait``,
    ``promote``, ``cancel``, ``on_done`` once its worker is free again, and ``took_ms``."""

    pool: WarmPool
    sentence: str
    timeout_s: float
    faced: bool = False
    urgent: bool = True
    on_done: Callable[[WarmRender], None] | None = None
    sample_rate: int = SAMPLE_RATE
    result: Rendered | None = None
    error: EngineError | None = None
    cancelled: bool = False
    worker: Worker | None = None
    took_ms: int = 0
    #: Set when the render has ended and its worker is back in the pool.
    done: threading.Event = field(default_factory=threading.Event)
    #: Set when the result is known or no longer wanted: what :meth:`wait` waits for.
    settled: threading.Event = field(default_factory=threading.Event)
    lock: threading.Lock = field(default_factory=threading.Lock)

    def start(self) -> WarmRender:
        threading.Thread(target=self._go, name="voice-render", daemon=True).start()
        return self

    def wait(self) -> Rendered | None:
        """The sentence, or ``None`` once it is cancelled (without waiting for the worker);
        raises :class:`EngineError`."""
        self.settled.wait()
        if self.cancelled:
            return None
        if self.error is not None:
            raise self.error
        return self.result

    def promote(self) -> None:
        with self.lock:
            if self.urgent:
                return
            self.urgent = True
            worker = self.worker
        if worker is not None:
            worker.set_urgent(True)

    def cancel(self) -> None:
        """The result is thrown away; the worker finishes the sentence below normal priority,
        out of the way of the sentence being waited for, and goes back."""
        with self.lock:
            if self.cancelled:
                return
            self.cancelled = True
            worker = self.worker
        self.settled.set()
        if worker is not None and not self.done.is_set():
            worker.set_urgent(False)

    def _go(self) -> None:
        began = time.monotonic()
        worker: Worker | None = None
        try:
            if not self.cancelled:
                worker = self.pool.acquire()
                with self.lock:
                    self.worker = worker
                    urgent = self.urgent
                if not self.cancelled:
                    began = time.monotonic()
                    raw, tokens = worker.render(self.sentence, self.timeout_s, urgent)
                    self.result = self._rendered(raw, tokens)
        except EngineError as exc:
            self.error = exc
        except Exception as exc:  # a bug in a render thread must still end the wait
            print(f"voice: a warm render failed: {exc!r}", file=sys.stderr, flush=True)
            self.error = EngineError(f"the render failed ({type(exc).__name__})")
        finally:
            self.took_ms = round((time.monotonic() - began) * 1000)
            if worker is not None:
                self.pool.release(worker)
            self.settled.set()
            self.done.set()
            if self.on_done is not None:
                try:
                    self.on_done(self)
                except Exception as exc:  # the pool's bookkeeping must not kill the thread
                    print(f"voice: after a render: {exc!r}", file=sys.stderr, flush=True)

    def _rendered(self, raw: bytes, tokens: list[list[int]]) -> Rendered:
        if not self.faced:
            return Rendered(pcm=raw)
        pcm, time_map = scale_silence(raw, self.sample_rate)
        return Rendered(pcm=pcm, tokens=tokens, raw_samples=len(raw) // 2, time_map=time_map)


# --- the worker process ---------------------------------------------------------------------------


def _own_priority(urgent: bool) -> None:
    """This worker's priority class (Windows only, as for the binary's runs)."""
    if sys.platform != "win32":
        return
    import ctypes

    kernel32 = ctypes.windll.kernel32  # type: ignore[attr-defined]
    kernel32.SetPriorityClass(
        kernel32.GetCurrentProcess(),
        NORMAL_PRIORITY_CLASS if urgent else BELOW_NORMAL_PRIORITY_CLASS,
    )


def _requests(sentences: queue.Queue[str | None]) -> None:
    """stdin, read while a sentence renders (``generate`` lets go of the GIL): a priority change
    takes effect at once, and a sentence's own priority is set as it is queued."""
    for raw in sys.stdin.buffer:
        try:
            message = json.loads(raw)
        except ValueError:
            message = None
        if not isinstance(message, dict):
            sentences.put("")
            continue
        if "urgent" in message:
            _own_priority(bool(message["urgent"]))
        if "text" in message:
            text = message["text"]
            sentences.put(text if isinstance(text, str) else "")
    sentences.put(None)


def _say(out: Any, message: dict[str, Any]) -> None:
    out.write(b"\n" + MARK + json.dumps(message, separators=(",", ":")).encode("ascii") + b"\n")
    out.flush()


def _tts(paths: KokoroPaths, threads: int, faced: bool) -> Any:
    import sherpa_onnx

    kokoro = sherpa_onnx.OfflineTtsKokoroModelConfig(
        model=str(paths.model),
        voices=str(paths.voices),
        tokens=str(paths.tokens),
        data_dir=str(paths.espeak),
        lexicon=str(paths.lexicon) if paths.lexicon.is_file() else "",
    )
    config = sherpa_onnx.OfflineTtsConfig(
        model=sherpa_onnx.OfflineTtsModelConfig(
            kokoro=kokoro, num_threads=max(threads, 1), debug=faced
        ),
        max_num_sentences=1,
    )
    return sherpa_onnx.OfflineTts(config)


def serve(argv: Sequence[str] | None = None) -> int:
    """The worker: load, warm, ready, then one sentence per stdin line until stdin closes."""
    parser = argparse.ArgumentParser(prog="shadowrunr_voice.warm")
    parser.add_argument("--engine-dir", required=True)
    parser.add_argument("--sid", type=int, default=3)
    parser.add_argument("--threads", type=int, default=2)
    parser.add_argument("--faced", action="store_true", help="print token ids, keep pauses whole")
    args = parser.parse_args(argv)
    out = sys.stdout.buffer
    began = time.monotonic()
    paths = paths_for(Path(args.engine_dir))
    missing = paths.missing(exe=False)
    if missing:
        _say(out, {"ready": False, "reason": f"{missing[0]} is missing from the engine dir"})
        return 1
    try:
        import numpy as np
        import sherpa_onnx
    except ImportError as exc:
        _say(out, {"ready": False, "reason": f"{exc.name or 'sherpa_onnx'} is not installed"})
        return 1
    try:
        tts = _tts(paths, args.threads, args.faced)
        config = sherpa_onnx.GenerationConfig()
        config.sid = args.sid
        config.speed = 1.0
        config.silence_scale = 1.0 if args.faced else SILENCE_SCALE
        tts.generate(WARM_UP, config)
    except Exception as exc:
        print(f"voice: the warm engine would not load: {exc!r}", file=sys.stderr, flush=True)
        _say(out, {"ready": False, "reason": f"the model would not load ({type(exc).__name__})"})
        return 1
    if tts.sample_rate != SAMPLE_RATE:
        _say(out, {"ready": False, "reason": f"the model speaks {tts.sample_rate} Hz"})
        return 1
    _say(out, {"ready": True, "load_ms": round((time.monotonic() - began) * 1000)})
    sentences: queue.Queue[str | None] = queue.Queue()
    threading.Thread(target=_requests, args=(sentences,), daemon=True).start()
    while (text := sentences.get()) is not None:
        if not text:
            _say(out, {"ok": False, "reason": "the request had no text"})
            continue
        try:
            audio = tts.generate(text, config)
            samples = np.asarray(audio.samples, dtype=np.float32)
            # sherpa's WAV writer: clipped to [-1, 1], times 32767, truncated.
            pcm = (np.clip(samples, -1.0, 1.0) * 32767).astype("<i2").tobytes()
        except Exception as exc:
            print(f"voice: a warm sentence failed: {exc!r}", file=sys.stderr, flush=True)
            _say(out, {"ok": False, "reason": f"the engine failed ({type(exc).__name__})"})
            continue
        _say(out, {"ok": True, "pcm": base64.b64encode(pcm).decode("ascii")})
    return 0


if __name__ == "__main__":
    raise SystemExit(serve())
