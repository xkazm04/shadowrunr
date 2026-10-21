# SPDX-License-Identifier: Apache-2.0
"""A stand-in for ``sherpa-onnx-offline-tts``: reads the same flags, writes a WAV.

The text decides what it does: ``FAIL`` exits 3, ``SLOW`` sleeps 30 s first (a render to cancel or
time out), ``PAUSE<n>`` sleeps ``n`` tenths of a second. Otherwise it writes ``0.1 s`` of a tone
per word at 24 kHz and, with ``--debug=1``, prints one token line. Every run appends its text to
the file named by ``FAKE_ENGINE_LOG``, so a test can see which sentences were started; with
``FAKE_ENGINE_SPANS`` it also appends ``start <text>`` and ``end <text>`` around its work there,
so a test can see which runs overlapped.
"""

from __future__ import annotations

import math
import os
import re
import sys
import time
import wave


def main(argv: list[str]) -> int:
    flags = {a.split("=", 1)[0]: a.split("=", 1)[1] for a in argv if a.startswith("--")}
    text = argv[-1]
    log = os.environ.get("FAKE_ENGINE_LOG")
    if log:
        with open(log, "a", encoding="utf-8") as f:
            f.write(text.strip() + "\n")
    spans = os.environ.get("FAKE_ENGINE_SPANS")
    if spans:
        with open(spans, "a", encoding="utf-8") as f:
            f.write(f"start {text.strip()}\n")
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
        with open(spans, "a", encoding="utf-8") as f:
            f.write(f"end {text.strip()}\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
