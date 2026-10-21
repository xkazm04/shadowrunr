# SPDX-License-Identifier: Apache-2.0
"""A stand-in for ``sherpa-onnx-offline-tts``: reads the same flags, writes a WAV.

The text decides what it does: ``FAIL`` exits 3, ``SLOW`` sleeps 30 s first (a render to cancel or
time out), ``PAUSE<n>`` sleeps ``n`` tenths of a second. Otherwise it writes ``0.1 s`` of a tone
per word at 24 kHz and, with ``--debug=1``, prints one token line. Every run appends its text to
the file named by ``FAKE_ENGINE_LOG``, so a test can see which sentences were started; with
``FAKE_ENGINE_SPANS`` it also appends ``start <text>`` and ``end <text>`` around its work there,
so a test can see which runs overlapped. ``HOLD<NAME>`` (capital letters) holds the run after its
``start`` line until a file ``<NAME>`` exists in the folder named by ``FAKE_ENGINE_GATES`` - an event
the test controls, so which runs overlap and which ends first never rests on the clock. A gate that
never opens fails the run after ``GATE_WAIT_S``.
"""

from __future__ import annotations

import math
import os
import re
import sys
import time
import wave
from pathlib import Path

GATE_WAIT_S = 15.0
# A byte far past any log's end: locking it serialises the appends without blocking the test's reads.
LOCK_AT = 1 << 30


def note(path: str, line: str) -> None:
    """Append one line. Runs released together append together. POSIX makes each O_APPEND write
    atomic; Windows seeks, then writes, so two runs can land on the same offset and one line is
    lost. There the append goes under a byte-range lock, which the OS drops if a run is killed."""
    data = (line + "\n").encode("utf-8")
    fd = os.open(path, os.O_WRONLY | os.O_APPEND | os.O_CREAT | getattr(os, "O_BINARY", 0))
    try:
        if os.name != "nt":
            os.write(fd, data)
            return
        import msvcrt

        os.lseek(fd, LOCK_AT, os.SEEK_SET)
        while True:
            try:
                msvcrt.locking(fd, msvcrt.LK_NBLCK, 1)
                break
            except OSError:  # held by another run (LK_LOCK would retry only once a second)
                time.sleep(0.002)
        try:
            os.write(fd, data)
        finally:
            os.lseek(fd, LOCK_AT, os.SEEK_SET)
            msvcrt.locking(fd, msvcrt.LK_UNLCK, 1)
    finally:
        os.close(fd)


def hold(text: str) -> bool:
    """Wait for this run's gate, when it names one. False when the gate never opened."""
    gate = re.search(r"HOLD([A-Z]+)", text)
    if not gate:
        return True
    folder = os.environ.get("FAKE_ENGINE_GATES")
    if not folder:
        print("fake engine: HOLD without FAKE_ENGINE_GATES", file=sys.stderr)
        return False
    path = Path(folder) / gate.group(1)
    deadline = time.monotonic() + GATE_WAIT_S
    while not path.exists():
        if time.monotonic() > deadline:
            print(f"fake engine: gate {gate.group(1)} never opened", file=sys.stderr)
            return False
        time.sleep(0.01)
    return True


def main(argv: list[str]) -> int:
    flags = {a.split("=", 1)[0]: a.split("=", 1)[1] for a in argv if a.startswith("--")}
    text = argv[-1]
    log = os.environ.get("FAKE_ENGINE_LOG")
    if log:
        note(log, text.strip())
    spans = os.environ.get("FAKE_ENGINE_SPANS")
    if spans:
        note(spans, f"start {text.strip()}")
    if not hold(text):
        return 4
    if "FAIL" in text:
        print("fake engine: told to fail", file=sys.stderr)
        return 3
    if "SLOW" in text:
        time.sleep(30)
    pause = re.search(r"PAUSE(\d+)", text)
    if pause:
        time.sleep(int(pause.group(1)) / 10)
    words = max(len(text.split()), 1)
    n = 2400 * words
    frames = b"".join(
        int(8000 * math.sin(2 * math.pi * 220 * i / 24000)).to_bytes(2, "little", signed=True)
        for i in range(n)
    )
    with wave.open(flags["--output-filename"], "wb") as wav:
        wav.setnchannels(1)
        wav.setsampwidth(2)
        wav.setframerate(24000)
        wav.writeframes(frames)
    if flags.get("--debug") == "1":
        print("0 50 " + " ".join("16" for _ in range(words)) + " 0")
    if spans:
        note(spans, f"end {text.strip()}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
