# SPDX-License-Identifier: Apache-2.0
"""A stand-in for a warm worker (``python -m shadowrunr_voice.warm``): the same pipe, no model.

It prints a line of chatter, then its ready line, then answers one ``{"text"}`` line at a time.
The text decides what it does, as in ``fake_engine.py``: ``FAIL`` answers an error, ``SLOW``
sleeps 30 s first, ``PAUSE<n>`` sleeps ``n`` tenths of a second, ``HOLD`` waits until the file
``FAKE_WARM_TEXT_GATE`` names exists, and ``DIE`` exits mid-sentence.
Otherwise it answers ``0.1 s`` of a tone per word and, with ``--faced``, prints one token line
before it. ``FAKE_WARM_LOAD=fail`` makes it fail its load. Every sentence appends its text to
``FAKE_ENGINE_LOG``, ``start``/``end`` lines to ``FAKE_ENGINE_SPANS``, and the worker's pid to
``FAKE_WARM_PIDS``, when those name files; each priority it is given (``urgent`` or ``below``)
goes to ``<pid>.txt`` in the directory ``FAKE_WARM_PRIORITY`` names.

Per spawn, for the pool's supervision: ``FAKE_WARM_SPAWNS`` names a directory where each worker
claims the next spawn number ``n`` by creating ``<n>.txt`` (holding its pid) before it loads.
``FAKE_WARM_FAIL_SPAWNS`` lists the spawn numbers that fail their load (``2``, ``2,4``, or ``3+``
for 3 and every later one). ``FAKE_WARM_SLOW_SPAWNS`` (the same form, every spawn when unset)
picks the spawns whose load is held: ``FAKE_WARM_LOAD_MS`` milliseconds, and then, when
``FAKE_WARM_LOAD_GATE`` names a file, until that file exists - an event a test controls, so
no assertion hangs on a clock.
"""

from __future__ import annotations

import base64
import json
import math
import os
import re
import sys
import time

# Run as a script, so its own folder is on sys.path.
from fake_engine import note as append_line

MARK = b"@@warm "


def say(message: dict[str, object]) -> None:
    sys.stdout.buffer.write(b"\n" + MARK + json.dumps(message).encode("ascii") + b"\n")
    sys.stdout.buffer.flush()


def note(env: str, text: str) -> None:
    """One line to the file ``env`` names. Two workers note at once, so this is
    ``fake_engine``'s locked append: a plain ``"a"`` append on Windows seeks, then writes, and
    two workers can land on one offset, losing a line."""
    path = os.environ.get(env)
    if path:
        append_line(path, text)


def claim_spawn() -> int:
    """This worker's spawn number (1, 2, ...), claimed atomically in ``FAKE_WARM_SPAWNS``; 0
    when that is unset."""
    folder = os.environ.get("FAKE_WARM_SPAWNS")
    if not folder:
        return 0
    n = 1
    while True:
        try:
            fd = os.open(os.path.join(folder, f"{n}.txt"), os.O_CREAT | os.O_EXCL | os.O_WRONLY)
        except FileExistsError:
            n += 1
            continue
        os.write(fd, str(os.getpid()).encode("ascii"))
        os.close(fd)
        return n


def picked(env: str, spawn: int, default: bool) -> bool:
    """Whether ``spawn`` is in the list ``env`` names (``2``, ``2,4``, ``3+``)."""
    spec = os.environ.get(env)
    if not spec:
        return default
    for item in spec.split(","):
        item = item.strip()
        if item.endswith("+") and spawn >= int(item[:-1]):
            return True
        if item and not item.endswith("+") and spawn == int(item):
            return True
    return False


def main(argv: list[str]) -> int:
    faced = "--faced" in argv
    spawn = claim_spawn()
    print("fake warm: loading the model (chatter the pool must ignore)", flush=True)
    if picked("FAKE_WARM_SLOW_SPAWNS", spawn, True):
        time.sleep(int(os.environ.get("FAKE_WARM_LOAD_MS") or 0) / 1000)
        gate = os.environ.get("FAKE_WARM_LOAD_GATE")
        while gate and not os.path.exists(gate):
            time.sleep(0.02)
    if os.environ.get("FAKE_WARM_LOAD") == "fail" or picked("FAKE_WARM_FAIL_SPAWNS", spawn, False):
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
        hold = os.environ.get("FAKE_WARM_TEXT_GATE")
        while "HOLD" in text and hold and not os.path.exists(hold):
            time.sleep(0.02)
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
