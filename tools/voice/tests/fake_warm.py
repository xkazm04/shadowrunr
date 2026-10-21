# SPDX-License-Identifier: Apache-2.0
"""A stand-in for a warm worker (``python -m shadowrunr_voice.warm``): the same pipe, no model.

It prints a line of chatter, then its ready line, then answers one ``{"text"}`` line at a time.
The text decides what it does, as in ``fake_engine.py``: ``FAIL`` answers an error, ``SLOW``
sleeps 30 s first, ``PAUSE<n>`` sleeps ``n`` tenths of a second, and ``DIE`` exits mid-sentence.
Otherwise it answers ``0.1 s`` of a tone per word and, with ``--faced``, prints one token line
before it. ``FAKE_WARM_LOAD=fail`` makes it fail its load. Every sentence appends its text to
``FAKE_ENGINE_LOG``, ``start``/``end`` lines to ``FAKE_ENGINE_SPANS``, and the worker's pid to
``FAKE_WARM_PIDS``, when those name files; each priority it is given (``urgent`` or ``below``)
goes to ``<pid>.txt`` in the directory ``FAKE_WARM_PRIORITY`` names.
"""

from __future__ import annotations

import base64
import json
import math
import os
import re
import sys
import time

MARK = b"@@warm "


def say(message: dict[str, object]) -> None:
    sys.stdout.buffer.write(b"\n" + MARK + json.dumps(message).encode("ascii") + b"\n")
    sys.stdout.buffer.flush()


def note(env: str, text: str) -> None:
    path = os.environ.get(env)
    if path:
        with open(path, "a", encoding="utf-8") as f:
            f.write(text + "\n")


def main(argv: list[str]) -> int:
    faced = "--faced" in argv
    print("fake warm: loading the model (chatter the pool must ignore)", flush=True)
    if os.environ.get("FAKE_WARM_LOAD") == "fail":
        say({"ready": False, "reason": "the model would not load (fake)"})
        return 1
    say({"ready": True, "load_ms": 7})
    for raw in sys.stdin.buffer:
        message = json.loads(raw)
        if "urgent" in message:
            priority = os.environ.get("FAKE_WARM_PRIORITY")
            if priority:
                with open(os.path.join(priority, f"{os.getpid()}.txt"), "a", encoding="utf-8") as f:
                    f.write(("urgent" if message["urgent"] else "below") + "\n")
        if "text" not in message:
            continue
        text = message["text"]
        note("FAKE_ENGINE_LOG", text.strip())
        note("FAKE_WARM_PIDS", str(os.getpid()))
        note("FAKE_ENGINE_SPANS", f"start {text.strip()}")
        if "DIE" in text:
            print("fake warm: dying", flush=True)
            return 9
        if "SLOW" in text:
            time.sleep(30)
        pause = re.search(r"PAUSE(\d+)", text)
        if pause:
            time.sleep(int(pause.group(1)) / 10)
        if "FAIL" in text:
            say({"ok": False, "reason": "the engine failed (fake)"})
            note("FAKE_ENGINE_SPANS", f"end {text.strip()}")
            continue
        words = max(len(text.split()), 1)
        pcm = b"".join(
            int(8000 * math.sin(2 * math.pi * 220 * i / 24000)).to_bytes(2, "little", signed=True)
            for i in range(2400 * words)
        )
        if faced:
            print("0 50 " + " ".join("16" for _ in range(words)) + " 0", flush=True)
        say({"ok": True, "pcm": base64.b64encode(pcm).decode("ascii")})
        note("FAKE_ENGINE_SPANS", f"end {text.strip()}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
