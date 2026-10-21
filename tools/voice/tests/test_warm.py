# SPDX-License-Identifier: Apache-2.0
"""The warm engine over a fake worker (``fake_warm.py``): chosen when it can run and fallen back
from when it cannot, sentences in order with their mouth shapes, a cancel that throws a render
away without killing its worker, and a worker that times out or dies replaced by a new one."""

from __future__ import annotations

import base64
import json
import os
import sys
import tempfile
import time
import unittest
from pathlib import Path
from typing import Any
from unittest import mock

from shadowrunr_voice.sidecar import Faces, Settings, Sidecar, hello
from shadowrunr_voice.warm import WarmPool, worker_argv

from .test_engine import FAKE, engine_dir
from .test_protocol import WAIT_S, FakeDurations, Sink

HERE = Path(__file__).resolve().parent
FAKE_WARM = [sys.executable, str(HERE / "fake_warm.py")]


class Warm(unittest.TestCase):
    def setUp(self) -> None:
        self.tmp = tempfile.TemporaryDirectory()
        self.root = engine_dir(Path(self.tmp.name))
        self.log = Path(self.tmp.name) / "engine-log.txt"
        self.pids = Path(self.tmp.name) / "warm-pids.txt"
        self.env = mock.patch.dict(
            os.environ, {"FAKE_ENGINE_LOG": str(self.log), "FAKE_WARM_PIDS": str(self.pids)}
        )
        self.env.start()
        self.sink = Sink()
        self.sidecars: list[Sidecar] = []
        self.pools: list[WarmPool] = []

    def tearDown(self) -> None:
        for sidecar in self.sidecars:
            sidecar.close()
        for pool in self.pools:
            pool.close()
        self.env.stop()
        self.tmp.cleanup()

    def settings(self, **kw: Any) -> Settings:
        return Settings(engine_dir=self.root, timeout_s=kw.pop("timeout_s", 20.0), **kw)

    def sidecar(
        self, *, workers: int = 2, timeout_s: float = 20.0, faces: Faces | None = None
    ) -> Sidecar:
        argv = [*FAKE_WARM, *(["--faced"] if faces is not None else [])]
        pool = WarmPool(argv, workers)
        self.pools.append(pool)
        pool.start(timeout_s=WAIT_S)
        settings = self.settings(timeout_s=timeout_s, workers=workers)
        sidecar = Sidecar(settings, self.sink.send, faces=faces, warm=pool)
        self.sidecars.append(sidecar)
        return sidecar

    def say(self, sidecar: Sidecar, ident: Any, text: str) -> None:
        sidecar.feed(json.dumps({"type": "say", "id": ident, "text": text}))

    def pid_lines(self) -> list[str]:
        return self.pids.read_text(encoding="utf-8").split() if self.pids.exists() else []

    # -- choosing the engine ---------------------------------------------------------------------

    def test_the_warm_engine_loads_before_the_hello_and_says_so(self) -> None:
        message, dead, faces, warm = hello(self.settings(workers=2, warm_command=FAKE_WARM))
        assert warm is not None
        self.pools.append(warm)
        self.assertIsNone(dead)
        self.assertIsNone(faces)
        self.assertTrue(all(w.alive for w in warm.workers))
        self.assertEqual(len(warm.workers), 2)
        self.assertIn("engine: warm (2 workers loaded in", message["reason"])
        self.assertIn("durations", message["reason"])  # what the voice lacks still comes first
        self.assertLess(message["reason"].index("durations"), message["reason"].index("engine"))

    def test_auto_without_the_packages_is_the_binary_and_says_why(self) -> None:
        with mock.patch("shadowrunr_voice.sidecar.missing_package", return_value="sherpa_onnx"):
            message, dead, _, warm = hello(self.settings())
        self.assertIsNone(warm)
        # No binary in the engine dir either: the voice is dead, named as before.
        self.assertIn("sherpa-onnx-offline-tts", str(dead))
        (self.root / "bin").mkdir()
        (self.root / "bin" / "sherpa-onnx-offline-tts.exe").write_bytes(b"")
        (self.root / "bin" / "sherpa-onnx-offline-tts").write_bytes(b"")
        with mock.patch("shadowrunr_voice.sidecar.missing_package", return_value="sherpa_onnx"):
            message, dead, _, warm = hello(self.settings())
        self.assertIsNone(dead)
        self.assertIsNone(warm)
        self.assertIn("engine: process (the warm engine is off: sherpa_onnx", message["reason"])

    def test_a_test_binary_or_an_explicit_process_is_the_binary(self) -> None:
        message, _, _, warm = hello(self.settings(command=FAKE))
        self.assertIsNone(warm)
        self.assertTrue(message["reason"].endswith("engine: process"))
        message, _, _, warm = hello(
            self.settings(command=FAKE, engine="process", warm_command=FAKE_WARM)
        )
        self.assertIsNone(warm)
        self.assertTrue(message["reason"].endswith("engine: process"))

    def test_a_warm_engine_that_will_not_load_falls_back_or_is_dead_when_demanded(self) -> None:
        with mock.patch.dict(os.environ, {"FAKE_WARM_LOAD": "fail"}):
            message, dead, _, warm = hello(self.settings(command=FAKE, warm_command=FAKE_WARM))
            self.assertIsNone(dead)
            self.assertIsNone(warm)
            self.assertIn("would not load: the model would not load (fake)", message["reason"])
            message, dead, _, warm = hello(
                self.settings(command=FAKE, warm_command=FAKE_WARM, engine="warm")
            )
        self.assertIsNone(warm)
        self.assertIn("the warm engine is not running", str(dead))
        self.assertEqual(message["reason"], dead)

    def test_a_worker_runs_this_interpreter_and_module_with_no_text(self) -> None:
        argv = worker_argv(Path("E"), 3, 0, True)
        self.assertEqual(argv[:3], [sys.executable, "-m", "shadowrunr_voice.warm"])
        self.assertEqual(argv[3:], ["--engine-dir", "E", "--sid", "3", "--threads", "1", "--faced"])

    # -- speaking --------------------------------------------------------------------------------

    def test_sentences_go_out_in_order_with_their_mouth_shapes(self) -> None:
        faces = Faces(FakeDurations(), {16: "ə", 50: " "}, 3)  # type: ignore[arg-type]
        sidecar = self.sidecar(workers=3, faces=faces)
        self.say(sidecar, 1, "The first one PAUSE8 waits here. The last one PAUSE4 of line one.")
        self.say(sidecar, 2, "Line two PAUSE4 starts early.")
        with sidecar._lock:
            renders = [r for line in sidecar._lines for r in line.renders]
        got = self.sink.wait_for(lambda m: len(m) == 3)
        self.assertEqual(
            [(m["type"], m["id"], m["part"], m["last"]) for m in got],
            [("audio", 1, 0, False), ("audio", 1, 1, True), ("audio", 2, 0, True)],
        )
        for audio in got:
            samples = len(base64.b64decode(audio["pcm"])) // 2
            self.assertEqual(samples, 2400 * len(audio["text"].split()))
            self.assertTrue(audio["visemes"])
            self.assertEqual(audio["visemes"][-1][2], round(samples * 1000 / 24_000))
        # Three workers, three sentences: each ran on a worker of its own, at once.
        self.assertEqual(len(renders), 3)
        self.assertEqual(len({id(r.worker) for r in renders}), 3)

    def test_a_failed_sentence_is_an_error_and_its_worker_stays(self) -> None:
        sidecar = self.sidecar(workers=1)
        self.say(sidecar, "x", "This one is fine. This one will FAIL now.")
        self.say(sidecar, "y", "The next line still speaks.")
        got = self.sink.wait_for(lambda m: len(m) == 3)
        self.assertEqual(
            [(m["type"], m["id"], m["part"]) for m in got],
            [("audio", "x", 0), ("error", "x", 1), ("audio", "y", 0)],
        )
        self.assertEqual(got[1]["reason"], "the engine failed (fake)")
        self.assertEqual(len(set(self.pid_lines())), 1)

    def test_cancel_throws_the_render_away_and_skips_the_queued_sentences(self) -> None:
        sidecar = self.sidecar(workers=1)
        self.say(sidecar, "x", "Said PAUSE10 too late. Queued behind it here.")
        self.say(sidecar, "z", "Never spoken either.")
        deadline = time.monotonic() + WAIT_S
        while not self.log.exists():
            self.assertLess(time.monotonic(), deadline)
            time.sleep(0.02)
        sidecar.feed(json.dumps({"type": "cancel", "ids": ["x", "z"]}))
        self.say(sidecar, "y", "Spoken after the cancel.")
        got = self.sink.wait_for(lambda m: len(m) == 1)
        time.sleep(0.3)
        self.assertEqual([(m["type"], m["id"]) for m in self.sink.messages], [("audio", "y")])
        self.assertEqual(got[0]["text"], "Spoken after the cancel.")
        said = self.log.read_text(encoding="utf-8")
        self.assertNotIn("Queued behind it here.", said)
        self.assertNotIn("Never spoken either.", said)
        # The worker was not killed: the same process finished the cancelled sentence and then
        # spoke the next line.
        pids = self.pid_lines()
        self.assertEqual(len(pids), 2)
        self.assertEqual(pids[0], pids[1])

    def test_the_sender_does_not_wait_for_a_cancelled_render(self) -> None:
        sidecar = self.sidecar(workers=2)
        self.say(sidecar, "x", "This is SLOW to say.")
        deadline = time.monotonic() + WAIT_S
        while not self.log.exists():
            self.assertLess(time.monotonic(), deadline)
            time.sleep(0.02)
        began = time.monotonic()
        sidecar.feed(json.dumps({"type": "cancel", "ids": ["x"]}))
        self.say(sidecar, "y", "On the other worker.")
        got = self.sink.wait_for(lambda m: len(m) == 1)
        self.assertLess(time.monotonic() - began, 10, "the sender waited for the slow render")
        self.assertEqual(got[0]["id"], "y")

    def test_a_render_past_the_timeout_kills_its_worker_and_the_next_starts_anew(self) -> None:
        sidecar = self.sidecar(workers=1, timeout_s=0.5)
        self.say(sidecar, "x", "This is SLOW to say.")
        self.say(sidecar, "y", "A new worker speaks this.")
        got = self.sink.wait_for(lambda m: len(m) == 2)
        self.assertEqual([(m["type"], m["id"]) for m in got], [("error", "x"), ("audio", "y")])
        self.assertIn("timed out after 500 ms", got[0]["reason"])
        pids = self.pid_lines()
        self.assertEqual(len(pids), 2)
        self.assertNotEqual(pids[0], pids[1])

    def test_a_worker_that_dies_is_an_error_and_is_replaced(self) -> None:
        sidecar = self.sidecar(workers=1)
        self.say(sidecar, "x", "This one will DIE now.")
        self.say(sidecar, "y", "A new worker speaks this.")
        got = self.sink.wait_for(lambda m: len(m) == 2)
        self.assertEqual([(m["type"], m["id"]) for m in got], [("error", "x"), ("audio", "y")])
        self.assertEqual(got[0]["reason"], "the warm engine stopped")
        pids = self.pid_lines()
        self.assertNotEqual(pids[0], pids[1])

    def test_the_head_of_line_runs_urgent_and_look_ahead_is_promoted(self) -> None:
        priority = Path(self.tmp.name) / "priority"
        priority.mkdir()
        os.environ["FAKE_WARM_PRIORITY"] = str(priority)
        sidecar = self.sidecar(workers=3)
        self.say(sidecar, 1, "The first one PAUSE5 here. The second one PAUSE5 here.")
        self.say(sidecar, 2, "Line two waits PAUSE3 its turn.")
        deadline = time.monotonic() + WAIT_S
        while sidecar._running < 3:
            self.assertLess(time.monotonic(), deadline)
            time.sleep(0.01)
        with sidecar._lock:
            one, two = sidecar._lines[0], sidecar._lines[1]
            started = [r.urgent for r in one.renders] + [r.urgent for r in two.renders]
        self.assertEqual(started, [True, False, False])
        self.sink.wait_for(lambda m: len(m) == 3)
        self.assertTrue(all(r.urgent for r in one.renders + two.renders))
        # Each worker was told: the head of line once, each look-ahead below, then raised.
        told = sorted(p.read_text(encoding="utf-8").split() for p in priority.iterdir())
        self.assertEqual(told, [["below", "urgent"], ["below", "urgent"], ["urgent"]])

    def test_a_cancelled_render_drops_below_normal_while_it_finishes(self) -> None:
        priority = Path(self.tmp.name) / "priority"
        priority.mkdir()
        os.environ["FAKE_WARM_PRIORITY"] = str(priority)
        sidecar = self.sidecar(workers=1)
        self.say(sidecar, "x", "Said PAUSE5 and dropped.")
        deadline = time.monotonic() + WAIT_S
        while not self.log.exists():
            self.assertLess(time.monotonic(), deadline)
            time.sleep(0.02)
        sidecar.feed(json.dumps({"type": "cancel", "ids": ["x"]}))
        self.say(sidecar, "y", "Next.")
        self.sink.wait_for(lambda m: len(m) == 1)
        (told,) = [p.read_text(encoding="utf-8").split() for p in priority.iterdir()]
        self.assertEqual(told, ["urgent", "below", "urgent"])

    def test_close_stops_every_worker(self) -> None:
        sidecar = self.sidecar(workers=2)
        self.say(sidecar, "a", "Hello there.")
        self.sink.wait_for(lambda m: len(m) == 1)
        pool = self.pools[0]
        sidecar.close()
        self.sidecars.remove(sidecar)
        self.assertFalse(any(w.alive for w in pool.workers))


if __name__ == "__main__":
    unittest.main()
