# SPDX-License-Identifier: Apache-2.0
"""Kokoro through ``sherpa-onnx-offline-tts``: one process per sentence, a WAV out, PCM in.

**The operator's install.** The engine binary and espeak-ng (GPL) are never bundled; the sidecar
calls what ``--engine-dir`` names: ``bin/sherpa-onnx-offline-tts[.exe]`` and ``kokoro/`` (model,
voices, tokens, espeak-ng data, the US-English lexicon when present).

**The arguments** are the ones Personas and athena run: ``--kokoro-model``, ``--kokoro-voices``,
``--kokoro-tokens``, ``--kokoro-data-dir``, ``--kokoro-lexicon`` when present,
``--num-threads`` (the pool's share of the cores), ``--sid``, ``--output-filename``, then the text. No shell, ever: the text is
one argument in a list, so nothing a model wrote can become a command. On Windows the run carries
``CREATE_NO_WINDOW``.

**Mouth shapes need the token ids and the uncut audio.** With visemes on the engine is also asked
for :data:`FACE_ARGS`: ``--debug=1`` prints each batch's token ids, and ``--tts-silence-scale=1``
keeps every pause whole. :func:`scale_silence` then replays sherpa's own pause cut sample for
sample, so the audio sent is the same audio the engine would have written by default, together
with a time map from the uncut audio to the cut one that the viseme spans go through.
"""

from __future__ import annotations

import codecs
import functools
import re
import subprocess
import sys
import tempfile
import threading
import time
import unicodedata
import wave
from array import array
from collections.abc import Callable, Sequence
from dataclasses import dataclass, field
from pathlib import Path

__all__ = [
    "ANSI_CODE_PAGE",
    "BELOW_NORMAL_PRIORITY_CLASS",
    "ENGINE_FILENAME",
    "FACE_ARGS",
    "NORMAL_PRIORITY_CLASS",
    "SAMPLE_RATE",
    "EngineError",
    "KokoroPaths",
    "Render",
    "Rendered",
    "build_args",
    "creation_flags",
    "narrow_safe",
    "paths_for",
    "read_pcm",
    "scale_silence",
    "spoken_entry",
    "token_lines",
    "voices_problem",
]

ENGINE_FILENAME = "sherpa-onnx-offline-tts" + (".exe" if sys.platform == "win32" else "")
#: What Kokoro speaks: 24 kHz, 16-bit, mono.
SAMPLE_RATE = 24_000
#: How much of a failed engine's stderr is logged.
STDERR_SNIPPET = 300

#: What the engine is asked for when visemes are on: the token ids, and the audio uncut.
FACE_ARGS: tuple[str, ...] = ("--debug=1", "--tts-silence-scale=1")
#: sherpa's default ``--tts-silence-scale``: a pause keeps a fifth of itself.
SILENCE_SCALE = 0.2
#: A pause is at least this long (``sample_rate * 0.2`` in ``ScaleSilence``).
PAUSE_S = 0.2
#: ``fabs(x) <= 0.01`` on the float sample, read off the 16-bit WAV the engine wrote with
#: ``trunc(x * 32767)``: ``|s| <= 327``.
QUIET_LEVEL = 327


class EngineError(Exception):
    """A sentence the engine did not render. The message is one line, with no local paths."""


#: Windows priority classes: a look-ahead run yields the CPU to the sentence being waited for.
BELOW_NORMAL_PRIORITY_CLASS = 0x4000
NORMAL_PRIORITY_CLASS = 0x20


def creation_flags(urgent: bool = True) -> int:
    """``CREATE_NO_WINDOW`` on Windows, so a run from a windowless daemon flashes no console, and
    ``BELOW_NORMAL_PRIORITY_CLASS`` for a run nobody is waiting for yet. 0 elsewhere."""
    if sys.platform != "win32":
        return 0
    flags = getattr(subprocess, "CREATE_NO_WINDOW", 0)
    return flags if urgent else flags | BELOW_NORMAL_PRIORITY_CLASS


def _set_priority(proc: subprocess.Popen[bytes], priority_class: int) -> None:
    """Change a running engine's priority class (Windows only; best effort)."""
    if sys.platform != "win32":
        return
    import ctypes

    handle = getattr(proc, "_handle", None)
    if handle is not None:
        ctypes.windll.kernel32.SetPriorityClass(int(handle), priority_class)  # type: ignore[attr-defined]


# --- the files ------------------------------------------------------------------------------------


@dataclass(frozen=True)
class KokoroPaths:
    exe: Path
    model: Path
    voices: Path
    tokens: Path
    espeak: Path
    lexicon: Path
    #: The model's own ``voices.bin``: the size a voices file must have.
    stock_voices: Path | None = None

    @property
    def own_voices(self) -> bool:
        """Whether the voices file is not the engine dir's own (a blend, :mod:`.blend`)."""
        return self.stock_voices is not None and self.voices != self.stock_voices

    def missing(self, *, exe: bool = True) -> list[str]:
        """What is not there, engine first, named relative to the engine dir. A voices file of
        the operator's own is :func:`voices_problem`'s."""
        gone: list[str] = []
        if exe and not self.exe.is_file():
            gone.append(f"bin/{self.exe.name}")
        for path in (self.model, self.voices, self.tokens):
            if path == self.voices and self.own_voices:
                continue
            if not path.is_file():
                gone.append(f"kokoro/{path.name}")
        if not self.espeak.is_dir():
            gone.append(f"kokoro/{self.espeak.name}/")
        return gone


def paths_for(engine_dir: Path, voices: Path | None = None) -> KokoroPaths:
    """Kokoro's files under ``engine_dir``; ``voices`` replaces its ``voices.bin`` for the
    engine and the mouth shapes alike."""
    model = engine_dir / "kokoro"
    stock = model / "voices.bin"
    return KokoroPaths(
        exe=engine_dir / "bin" / ENGINE_FILENAME,
        model=model / "model.onnx",
        voices=voices if voices is not None else stock,
        tokens=model / "tokens.txt",
        espeak=model / "espeak-ng-data",
        lexicon=model / "lexicon-us-en.txt",
        stock_voices=stock,
    )


#: One speaker's style rows in a voices file: 510 rows of 256 float32.
SPEAKER_BYTES = 510 * 256 * 4
#: kokoro-multi-lang-v1_0's speakers: a voices file's size when the stock one is not there.
SPEAKERS = 53


def voices_problem(paths: KokoroPaths, sid: int) -> str | None:
    """Why the voices file cannot speak ``sid``, by its name and never its path, or ``None``.

    sherpa-onnx refuses a voices file whose size is not the model's speaker count, so a blend
    must be the stock file's size exactly; the sid must be one of its speakers."""
    name = paths.voices.name
    if not paths.voices.is_file():
        return f"the voices file {name} is missing"
    size = paths.voices.stat().st_size
    stock = paths.stock_voices
    want = SPEAKERS * SPEAKER_BYTES
    if stock is not None and stock.is_file() and stock.stat().st_size > 0:
        want = stock.stat().st_size
    if size != want:
        return f"the voices file {name} is {size} bytes; the model's voices.bin is {want}"
    speakers = size // SPEAKER_BYTES
    if not 0 <= sid < speakers:
        return f"sid {sid} is not one of the {speakers} speakers in the voices file {name}"
    return None


# --- the argument ---------------------------------------------------------------------------------


def _ansi_code_page() -> int | None:
    """The code page a narrow ``main`` reads its command line in: ``GetACP()`` on Windows."""
    if sys.platform != "win32":
        return None
    import ctypes

    return int(ctypes.windll.kernel32.GetACP())  # type: ignore[attr-defined]


#: The running process's ANSI code page, or ``None`` off Windows. A child inherits it, and
#: ``sherpa-onnx-offline-tts`` has a narrow ``main``.
ANSI_CODE_PAGE = _ansi_code_page()


@functools.lru_cache(maxsize=4096)
def _carried(char: str, code_page: int) -> bool:
    """Whether ``char`` survives ``code_page`` exactly: encoded with no best fit, then decoded."""
    encode = getattr(codecs, "code_page_encode", None)
    decode = getattr(codecs, "code_page_decode", None)
    try:
        if encode is not None and decode is not None:
            raw = encode(code_page, char, "strict")[0]
            back = decode(code_page, raw, "strict", True)[0]
        else:
            back = char.encode(f"cp{code_page}").decode(f"cp{code_page}")
    except (LookupError, OSError):  # no such code page here
        return char.isascii()
    except UnicodeError:
        return False
    return bool(back == char)


def _fold(char: str, code_page: int) -> str:
    """One character the code page cannot carry: its NFKC form when carried, else its NFKD form
    without combining marks when carried, else one space so two words never run together."""
    for form in (
        unicodedata.normalize("NFKC", char),
        "".join(
            c
            for c in unicodedata.normalize("NFKD", char)
            if unicodedata.category(c) not in ("Mn", "Me")
        ),
    ):
        if form and all(_carried(c, code_page) for c in form):
            return form
    return " "


def narrow_safe(text: str, code_page: int | None) -> str:
    """``text`` with every character ``code_page`` cannot carry exactly replaced.

    Windows builds a narrow child's command line with ``WideCharToMultiByte``, which best-fits a
    character the code page lacks: U+FF02 becomes ``"`` and U+FF3C a backslash, so the child would
    split its arguments where ``list2cmdline`` quoted one. Once every character is carried, the
    narrow line decodes back to the wide one. ``None`` (off Windows) is the identity."""
    if code_page is None:
        return text
    return "".join(char if _carried(char, code_page) else _fold(char, code_page) for char in text)


def spoken_entry(text: str, code_page: int | None = ANSI_CODE_PAGE) -> str:
    """The sentence as the engine's one positional argument, always one space first: a line such
    as ``--output-filename=x`` must never be read as a flag, and espeak ignores the space."""
    return f" {narrow_safe(text, code_page)}"


def build_args(
    paths: KokoroPaths,
    sid: int,
    output: Path,
    text: str,
    code_page: int | None = ANSI_CODE_PAGE,
    extra: Sequence[str] = (),
    threads: int = 2,
) -> list[str]:
    """The engine's flags, text last and positional. No executable. ``extra`` goes just before
    the text: :data:`FACE_ARGS` when visemes are on."""
    args = [
        f"--kokoro-model={paths.model}",
        f"--kokoro-voices={paths.voices}",
        f"--kokoro-tokens={paths.tokens}",
        f"--kokoro-data-dir={paths.espeak}",
    ]
    if paths.lexicon.is_file():
        args.append(f"--kokoro-lexicon={paths.lexicon}")
    args += [
        f"--num-threads={max(threads, 1)}",
        f"--sid={sid}",
        f"--output-filename={output}",
        *extra,
        spoken_entry(text, code_page),
    ]
    return args


def read_pcm(wav_path: Path, sample_rate: int = SAMPLE_RATE) -> bytes:
    """The frames of a 16-bit mono WAV at ``sample_rate``; anything else is an engine fault."""
    try:
        with wave.open(str(wav_path), "rb") as wav:
            if wav.getnchannels() != 1 or wav.getsampwidth() != 2:
                raise EngineError(
                    f"the engine wrote {wav.getnchannels()} channel(s) of "
                    f"{wav.getsampwidth() * 8} bit audio; expected mono 16-bit"
                )
            if wav.getframerate() != sample_rate:
                raise EngineError(
                    f"the engine wrote {wav.getframerate()} Hz; expected {sample_rate}"
                )
            return wav.readframes(wav.getnframes())
    except (OSError, EOFError, wave.Error) as exc:
        raise EngineError(f"the engine's output was not a WAV ({type(exc).__name__})") from None


# --- sherpa's arithmetic, replayed ----------------------------------------------------------------

_TOKEN_LINE = re.compile(r"^0(?: \d+)+ 0$")

TimeMap = list[tuple[int, int, int, int]]


def token_lines(output: str) -> list[list[int]]:
    """The token-id sequences ``--debug=1`` printed, one per batch the engine ran: each batch's
    ids padded with ``0`` on both sides, on a line of their own."""
    return [
        [int(x) for x in line.split()]
        for line in (raw.strip() for raw in output.splitlines())
        if _TOKEN_LINE.match(line)
    ]


def scale_silence(
    pcm: bytes, sample_rate: int, scale: float = SILENCE_SCALE
) -> tuple[bytes, TimeMap]:
    """sherpa's ``GeneratedAudio::ScaleSilence`` over 16-bit PCM, and the time map it implies.

    A run of quiet samples at least :data:`PAUSE_S` long ending in a loud sample, or longer than
    that at the very end, is a pause, and only its first ``int(len * scale)`` samples are kept.
    The map is ``(raw_a, raw_b, out_a, out_b)`` segments: speech maps one to one, and a whole
    pause maps linearly onto the part of it that is kept."""
    samples = array("h")
    samples.frombytes(pcm[: len(pcm) - len(pcm) % 2])
    if sys.byteorder != "little":
        samples.byteswap()
    n = len(samples)
    threshold = int(sample_rate * PAUSE_S)
    quiet = bytes(1 if -QUIET_LEVEL <= s <= QUIET_LEVEL else 0 for s in samples)
    intervals: list[tuple[int, int]] = []
    for run in re.finditer(b"\x01+", quiet):
        a, b = run.span()
        if b < n:
            if b - a >= threshold:
                intervals.append((a, b))
        elif n - a > threshold:
            intervals.append((a, n))
    if not intervals:
        return pcm, [(0, n, 0, n)]
    out = bytearray()
    time_map: TimeMap = []
    pos = 0
    at = 0
    for a, b in intervals:
        out += pcm[pos * 2 : a * 2]
        time_map.append((pos, a, at, at + (a - pos)))
        at += a - pos
        kept = int((b - a) * scale)
        out += pcm[a * 2 : (a + kept) * 2]
        time_map.append((a, b, at, at + kept))
        at += kept
        pos = b
    if pos < n:
        out += pcm[pos * 2 : n * 2]
        time_map.append((pos, n, at, at + (n - pos)))
    return bytes(out), [seg for seg in time_map if seg[1] > seg[0]]


# --- one sentence ---------------------------------------------------------------------------------


@dataclass
class Rendered:
    """One sentence's audio as it is played, and what the visemes need of the uncut run."""

    pcm: bytes
    tokens: list[list[int]] = field(default_factory=list)
    raw_samples: int = 0
    time_map: TimeMap = field(default_factory=list)


@dataclass
class Render:
    """One sentence on its own thread: the process, its WAV, and a way to stop it early.

    ``command`` replaces the engine executable (a test's ``[python, fake_engine.py]``).
    ``on_done`` is called on the render's thread once it has ended, however it ended.
    ``urgent`` is false for a look-ahead run: on Windows it starts below normal priority, and
    :meth:`promote` raises it once the sentence is the one being waited for."""

    paths: KokoroPaths
    sid: int
    sentence: str
    timeout_s: float
    faced: bool = False
    command: Sequence[str] | None = None
    threads: int = 2
    urgent: bool = True
    on_done: Callable[[Render], None] | None = None
    sample_rate: int = SAMPLE_RATE
    result: Rendered | None = None
    error: EngineError | None = None
    cancelled: bool = False
    proc: subprocess.Popen[bytes] | None = None
    #: How long the engine run took, for the operator's log.
    took_ms: int = 0
    done: threading.Event = field(default_factory=threading.Event)
    lock: threading.Lock = field(default_factory=threading.Lock)

    def start(self) -> Render:
        threading.Thread(target=self._go, name="voice-render", daemon=True).start()
        return self

    def wait(self) -> Rendered | None:
        """The sentence, or ``None`` when it was cancelled; raises :class:`EngineError`."""
        self.done.wait()
        if self.cancelled:
            return None
        if self.error is not None:
            raise self.error
        return self.result

    def promote(self) -> None:
        """This sentence is the one being waited for now: full priority from here on."""
        with self.lock:
            if self.urgent:
                return
            self.urgent = True
            proc = self.proc
        if proc is not None and proc.poll() is None:
            try:
                _set_priority(proc, NORMAL_PRIORITY_CLASS)
            except OSError:
                pass

    def cancel(self) -> None:
        with self.lock:
            self.cancelled = True
            proc = self.proc
        if proc is not None and proc.poll() is None:
            proc.kill()

    def _go(self) -> None:
        began = time.monotonic()
        try:
            self.result = self._run()
        except EngineError as exc:
            self.error = exc
        except Exception as exc:  # a bug in a render thread must still end the wait
            print(f"voice: a render failed: {exc!r}", file=sys.stderr, flush=True)
            self.error = EngineError(f"the render failed ({type(exc).__name__})")
        finally:
            self.took_ms = round((time.monotonic() - began) * 1000)
            self.done.set()
            if self.on_done is not None:
                try:
                    self.on_done(self)
                except Exception as exc:  # the pool's bookkeeping must not kill the thread
                    print(f"voice: after a render: {exc!r}", file=sys.stderr, flush=True)

    def _run(self) -> Rendered | None:
        exe = list(self.command) if self.command is not None else [str(self.paths.exe)]
        with tempfile.TemporaryDirectory(prefix="shadowrunr-voice-") as tmp:
            output = Path(tmp) / "out.wav"
            argv = [
                *exe,
                *build_args(
                    self.paths,
                    self.sid,
                    output,
                    self.sentence,
                    extra=FACE_ARGS if self.faced else (),
                    threads=self.threads,
                ),
            ]
            with self.lock:
                if self.cancelled:
                    return None
                try:
                    self.proc = subprocess.Popen(
                        argv,
                        stdin=subprocess.DEVNULL,
                        stdout=subprocess.PIPE,
                        stderr=subprocess.PIPE,
                        creationflags=creation_flags(self.urgent),
                    )
                except OSError as exc:
                    raise EngineError(
                        f"the engine would not start ({type(exc).__name__})"
                    ) from None
            proc = self.proc
            try:
                stdout, stderr = proc.communicate(timeout=self.timeout_s)
            except subprocess.TimeoutExpired:
                proc.kill()
                proc.communicate()
                raise EngineError(
                    f"the render timed out after {round(self.timeout_s * 1000)} ms"
                ) from None
            if self.cancelled:
                return None
            if proc.returncode != 0:
                # The engine echoes its arguments, local paths included: its words go to this
                # process's stderr for the operator, never into the reason that leaves it.
                snippet = " ".join(stderr.decode("utf-8", "replace").split())[-STDERR_SNIPPET:]
                print(f"voice: engine stderr: {snippet}", file=sys.stderr, flush=True)
                raise EngineError(f"the engine exited with {proc.returncode}")
            raw = read_pcm(output, self.sample_rate)
        if not self.faced:
            return Rendered(pcm=raw)
        pcm, time_map = scale_silence(raw, self.sample_rate)
        printed = stdout.decode("utf-8", "replace") + "\n" + stderr.decode("utf-8", "replace")
        return Rendered(
            pcm=pcm,
            tokens=token_lines(printed),
            raw_samples=len(raw) // 2,
            time_map=time_map,
        )
