# SPDX-License-Identifier: Apache-2.0
"""The render pool over the fake engine: sentences render together, across lines, and still go
out in order; a cancel or a failure kills what is running and frees its slot."""

from __future__ import annotations

import json
import os
import tempfile
import time
import unittest
from pathlib import Path
from typing import Any

from shadowrunr_voice.sidecar import DEFAULT_THREADS, Settings, Sidecar

from .test_engine import FAKE, engine_dir
from .test_protocol import WAIT_S, Sink


class Pool(unittest.TestCase):
    def setUp(self) -> None:
        self.tmp = tempfile.TemporaryDirectory()
        self.root = engine_dir(Path(self.tmp.name))
        self.log = Path(self.tmp.name) / "engine-log.txt"
        self.spans = Path(self.tmp.name) / "engine-spans.txt"
        os.environ["FAKE_ENGINE_LOG"] = str(self.log)
        os.environ["FAKE_ENGINE_SPANS"] = str(self.spans)
        self.sink = Sink()
        self.sidecars: list[Sidecar] = []

    def tearDown(self) -> None:
        for sidecar in self.sidecars:
            sidecar.close()
        del os.environ["FAKE_ENGINE_LOG"]
        del os.environ["FAKE_ENGINE_SPANS"]
        self.tmp.cleanup()

    def sidecar(self, workers: int) -> Sidecar:
        settings = Settings(engine_dir=self.root, timeout_s=20.0, command=FAKE, workers=workers)
        sidecar = Sidecar(settings, self.sink.send)
        self.sidecars.append(sidecar)
        return sidecar

    def say(self, sidecar: Sidecar, ident: Any, text: str) -> None:
        sidecar.feed(json.dumps({"type": "say", "id": ident, "text": text}))

    def events(self) -> list[str]:
        return self.spans.read_text(encoding="utf-8").splitlines()

    def idle(self, sidecar: Sidecar) -> None:
        deadline = time.monotonic() + WAIT_S
        while sidecar._running and time.monotonic() < deadline:
            time.sleep(0.02)
        self.assertEqual(sidecar._running, 0, "a render is still running")

    def test_lines_overlap_in_the_pool_and_go_out_in_order(self) -> None:
        sidecar = self.sidecar(workers=3)
        # Line 1's first sentence is the slowest: everything else finishes before it.
        self.say(sidecar, 1, "The first one PAUSE20 waits. The last one PAUSE10 of line one.")
        self.say(sidecar, 2, "Line two PAUSE5 starts early.")
        got = self.sink.wait_for(lambda m: len(m) == 3)
        self.assertEqual(
            [(m["type"], m["id"], m["part"], m["last"]) for m in got],
            [("audio", 1, 0, False), ("audio", 1, 1, True), ("audio", 2, 0, True)],
        )
        events = self.events()
        # Line 2's first sentence started while line 1's last one was still rendering.
        self.assertLess(
            events.index("start Line two PAUSE5 starts early."),
            events.index("end The last one PAUSE10 of line one."),
        )
        # And all three ran at once: every start comes before the first end.
        first_end = next(i for i, e in enumerate(events) if e.startswith("end "))
        self.assertEqual(sum(1 for e in events[:first_end] if e.startswith("start ")), 3)

    def test_the_pool_never_runs_more_than_its_workers(self) -> None:
        sidecar = self.sidecar(workers=2)
        self.say(sidecar, "a", "One sentence PAUSE3 here. Two sentence PAUSE3 here.")
        self.say(sidecar, "b", "Three sentence PAUSE3 here. Four sentence PAUSE3 here.")
        got = self.sink.wait_for(lambda m: len(m) == 4)
        self.assertEqual(
            [(m["id"], m["part"]) for m in got], [("a", 0), ("a", 1), ("b", 0), ("b", 1)]
        )
        running = peak = 0
        for e in self.events():
            running += 1 if e.startswith("start ") else -1
            peak = max(peak, running)
        self.assertEqual(peak, 2)
        # Sentences start in arrival order: all of line a before line b.
        starts = [e[len("start ") :] for e in self.events() if e.startswith("start ")]
        self.assertEqual(
            set(starts[:2]), {"One sentence PAUSE3 here.", "Two sentence PAUSE3 here."}
        )

    def test_cancel_mid_pool_kills_the_runs_and_frees_their_slots(self) -> None:
        sidecar = self.sidecar(workers=2)
        # Both slots go to "x"; "y" waits for a slot, "z" waits behind it.
        self.say(sidecar, "x", "This is SLOW to say. Also SLOW to say here.")
        self.say(sidecar, "y", "Spoken after the cancel.")
        self.say(sidecar, "z", "Cancelled while it waits.")
        deadline = time.monotonic() + WAIT_S
        while self.log.exists() is False or len(self.log.read_text("utf-8").splitlines()) < 2:
            self.assertLess(time.monotonic(), deadline)
            time.sleep(0.02)
        began = time.monotonic()
        sidecar.feed(json.dumps({"type": "cancel", "ids": ["x", "z"]}))
        got = self.sink.wait_for(lambda m: len(m) == 1)
        self.assertLess(time.monotonic() - began, 10, "the slow runs were not killed")
        self.idle(sidecar)
        time.sleep(0.2)
        self.assertEqual(
            [(m["type"], m["id"]) for m in self.sink.messages], [("audio", "y")]
        )
        self.assertEqual(got[0]["text"], "Spoken after the cancel.")
        self.assertNotIn("Cancelled while it waits.", self.log.read_text(encoding="utf-8"))

    def test_a_failed_sentence_kills_the_rest_of_its_line_in_the_pool(self) -> None:
        sidecar = self.sidecar(workers=3)
        self.say(sidecar, "x", "This one is fine. This one will FAIL now. This is SLOW to say.")
        self.say(sidecar, "y", "The next line still speaks.")
        began = time.monotonic()
        got = self.sink.wait_for(lambda m: len(m) == 3)
        self.assertLess(time.monotonic() - began, 10, "the slow run was not killed")
        self.assertEqual(
            [(m["type"], m["id"], m["part"]) for m in got],
            [("audio", "x", 0), ("error", "x", 1), ("audio", "y", 0)],
        )
        self.idle(sidecar)

    def test_the_head_of_line_runs_urgent_and_look_ahead_waits_its_turn(self) -> None:
        sidecar = self.sidecar(workers=3)
        self.say(sidecar, 1, "The first one PAUSE5 here. The second one PAUSE5 here.")
        self.say(sidecar, 2, "Line two waits its turn.")
        deadline = time.monotonic() + WAIT_S
        while sidecar._running < 3:
            self.assertLess(time.monotonic(), deadline)
            time.sleep(0.01)
        with sidecar._lock:
            one, two = sidecar._lines[0], sidecar._lines[1]
            started = [r.urgent for r in one.renders] + [r.urgent for r in two.renders]
        # Only the sentence the sender waits for runs at full priority.
        self.assertEqual(started, [True, False, False])
        self.sink.wait_for(lambda m: len(m) == 3)
        # Each look-ahead run was promoted once it was the one waited for.
        self.assertTrue(all(r.urgent for r in one.renders + two.renders))

    def test_threads_default_to_two_per_run(self) -> None:
        sidecar = self.sidecar(workers=3)
        self.assertEqual(sidecar.threads, DEFAULT_THREADS)
        settings = Settings(engine_dir=self.root, command=FAKE, workers=2, threads=0)
        one = Sidecar(settings, self.sink.send)
        self.sidecars.append(one)
        self.assertEqual(one.threads, 1)

if __name__ == "__main__":
    unittest.main()
