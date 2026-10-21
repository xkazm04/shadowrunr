# SPDX-License-Identifier: Apache-2.0
"""The warm engine over a fake worker (``fake_warm.py``): chosen when it can run and fallen back
from when it cannot, sentences in order with their mouth shapes, a cancel that throws a render
away without killing its worker, and a worker that times out or dies replaced by a new one.

The pool supervises its workers: it starts with whatever loads, respawns a dead worker in the
background from the moment it dies, quarantines a slot whose loads keep failing, and lends out
only live workers. Those cases wait on events (a spawn file, a gate file the test opens, the
pool's slot states), never on a clock."""

from __future__ import annotations

import base64
import json
import os
import sys
import tempfile
import threading
import time
import unittest
from pathlib import Path
from typing import Any
from unittest import mock

from shadowrunr_voice.sidecar import Faces, Settings, Sidecar, hello
from shadowrunr_voice import warm as warm_module
from shadowrunr_voice.warm import WarmPool, Worker, worker_argv

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
        # Respawn at once in tests: the backoff is a production pacing, not what is tested.
        self.backoff = mock.patch.object(warm_module, "RESPAWN_BACKOFF_S", (0.0,), create=True)
        self.backoff.start()
        self.sink = Sink()
        self.sidecars: list[Sidecar] = []
        self.pools: list[WarmPool] = []

    def tearDown(self) -> None:
        for sidecar in self.sidecars:
            sidecar.close()
        for pool in self.pools:
            pool.close()
        self.backoff.stop()
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

    def wait_until(self, done: Any, what: str) -> None:
        """Polls for an event; the deadline only bounds a failure, it is not what is asserted."""
        deadline = time.monotonic() + WAIT_S
        while not done():
            self.assertLess(time.monotonic(), deadline, f"never happened: {what}")
            time.sleep(0.02)

    @staticmethod
    def states(pool: WarmPool) -> dict[str, str]:
        """Each slot's state (``live``, ``loading``, ``quarantined``); empty while the pool
        cannot tell."""
        states = getattr(pool, "states", None)
        return states() if callable(states) else {}

    def spawns(self, **knobs: str) -> Path:
        """Numbered spawns for fake_warm, with its per-spawn knobs set."""
        folder = Path(self.tmp.name) / "spawns"
        folder.mkdir()
        os.environ["FAKE_WARM_SPAWNS"] = str(folder)
        os.environ.update(knobs)
        return folder

    @staticmethod
    def spawned(folder: Path) -> int:
        return len(list(folder.glob("*.txt")))

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

    def test_a_partial_start_keeps_the_warm_engine_and_names_its_workers(self) -> None:
        self.spawns(FAKE_WARM_FAIL_SPAWNS="2")
        settings = self.settings(workers=3, warm_command=FAKE_WARM, engine="warm")
        message, dead, _, warm = hello(settings)
        self.assertIsNotNone(warm, "one worker that would not load threw the pool away")
        assert warm is not None
        self.pools.append(warm)
        self.assertIsNone(dead)
        self.assertIn("engine: warm (", message["reason"])
        self.assertIn("2 of 3 workers", message["reason"])
        sidecar = Sidecar(settings, self.sink.send, warm=warm)
        self.sidecars.append(sidecar)
        self.say(sidecar, "a", "Two workers still speak.")
        got = self.sink.wait_for(lambda m: len(m) == 1)
        self.assertEqual([(m["type"], m["id"]) for m in got], [("audio", "a")])

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

    # -- supervision -----------------------------------------------------------------------------

    def test_a_dead_worker_is_respawned_at_its_death_not_at_the_next_demand(self) -> None:
        gate = Path(self.tmp.name) / "gate"
        hold = Path(self.tmp.name) / "hold"
        folder = self.spawns(
            FAKE_WARM_SLOW_SPAWNS="3+", FAKE_WARM_LOAD_GATE=str(gate), FAKE_WARM_TEXT_GATE=str(hold)
        )
        sidecar = self.sidecar(workers=2)
        pool = self.pools[0]
        self.say(sidecar, "x", "This one will DIE now.")
        got = self.sink.wait_for(lambda m: len(m) == 1)
        self.assertEqual((got[0]["type"], got[0]["reason"]), ("error", "the warm engine stopped"))
        # Nothing more is asked for: the replacement starts loading on its own, at the death.
        self.wait_until(lambda: (folder / "3.txt").exists(), "a respawn with no demand")
        self.assertEqual(sorted(self.states(pool).values()), ["live", "loading"])
        # While it loads (held until the gate opens), a sentence goes to the live worker and
        # never waits for the load.
        self.say(sidecar, "y", "The live worker speaks this.")
        got = self.sink.wait_for(lambda m: len(m) == 2)
        self.assertEqual((got[1]["type"], got[1]["id"]), ("audio", "y"))
        self.assertFalse(gate.exists())
        gate.touch()
        self.wait_until(lambda: sorted(self.states(pool).values()) == ["live", "live"], "ready")
        # Both workers are back: two sentences at once render on two processes, the new one too.
        # The first is held until the second has started, so one worker cannot take both.
        started = len(self.pid_lines())
        self.say(sidecar, "z", "The first one HOLD waits here. The second one is spoken here.")
        self.wait_until(lambda: len(self.pid_lines()) == started + 2, "both sentences started")
        hold.touch()
        self.sink.wait_for(lambda m: len(m) == 4)
        respawned = (folder / "3.txt").read_text(encoding="utf-8").strip()
        self.assertIn(respawned, self.pid_lines()[-2:])
        self.assertEqual(len(set(self.pid_lines()[-2:])), 2)

    def test_a_crash_looping_slot_is_quarantined_and_the_burst_runs_on_the_live_one(self) -> None:
        folder = self.spawns(FAKE_WARM_FAIL_SPAWNS="3+")
        sidecar = self.sidecar(workers=2)
        pool = self.pools[0]
        self.say(sidecar, "x", "This one will DIE now.")
        self.sink.wait_for(lambda m: len(m) == 1)
        self.wait_until(
            lambda: sorted(self.states(pool).values()) == ["live", "quarantined"], "quarantine"
        )
        # The crash budget's loads were tried, and then no more.
        tried = self.spawned(folder)
        self.assertEqual(tried, 2 + warm_module.CRASH_BUDGET)
        words = ("one", "two", "three", "four", "five", "six")
        self.say(sidecar, "b", " ".join(f"Sentence {word} is here." for word in words))
        got = self.sink.wait_for(lambda m: len(m) == 7)
        after = got[1:]
        self.assertEqual([m["type"] for m in after], ["audio"] * 6)
        self.assertEqual([m["part"] for m in after], list(range(6)))
        self.assertEqual(len(set(self.pid_lines()[1:])), 1)
        self.assertEqual(self.spawned(folder), tried, "a quarantined slot was loaded again")

    def test_with_every_worker_quarantined_a_say_fails_at_once_without_a_reload(self) -> None:
        folder = self.spawns(FAKE_WARM_FAIL_SPAWNS="2+")
        sidecar = self.sidecar(workers=1)
        pool = self.pools[0]
        self.say(sidecar, "x", "This one will DIE now.")
        self.sink.wait_for(lambda m: len(m) == 1)
        self.wait_until(lambda: list(self.states(pool).values()) == ["quarantined"], "quarantine")
        tried = self.spawned(folder)
        self.say(sidecar, "y", "Is anyone there?")
        got = self.sink.wait_for(lambda m: len(m) == 2)
        self.assertEqual(
            (got[1]["type"], got[1]["id"], got[1]["reason"]),
            ("error", "y", "the warm engine is not running"),
        )
        self.assertEqual(self.spawned(folder), tried, "the say tried a reload")

    def test_acquire_never_loads_a_worker_on_the_acquiring_thread(self) -> None:
        pool = WarmPool(FAKE_WARM, 1)
        self.pools.append(pool)
        pool.start(timeout_s=WAIT_S)
        callers: list[threading.Thread] = []
        original = Worker.ready

        def spy(worker: Worker, *args: Any, **kwargs: Any) -> int:
            callers.append(threading.current_thread())
            return original(worker, *args, **kwargs)

        got: list[Any] = []

        def acquire() -> None:
            try:
                got.append(pool.acquire())
            except Exception as exc:  # recorded, asserted below
                got.append(exc)

        with mock.patch.object(Worker, "ready", spy):
            proc = pool.workers[0].proc
            assert proc is not None
            proc.kill()
            proc.wait(timeout=WAIT_S)
            taker = threading.Thread(target=acquire, name="voice-test-acquire")
            taker.start()
            taker.join(WAIT_S)
            self.assertFalse(taker.is_alive(), "acquire never got the respawned worker")
        self.assertEqual(len(got), 1)
        self.assertIsInstance(got[0], Worker)
        self.assertTrue(got[0].alive)
        pool.release(got[0])
        self.assertTrue(callers, "the dead worker was never loaded again")
        self.assertNotIn(taker, callers, "acquire loaded the model on the render's thread")

    def test_close_kills_and_reaps_a_respawn_mid_load(self) -> None:
        gate = Path(self.tmp.name) / "never"
        folder = self.spawns(FAKE_WARM_SLOW_SPAWNS="2+", FAKE_WARM_LOAD_GATE=str(gate))
        pool = WarmPool(FAKE_WARM, 1)
        self.pools.append(pool)
        pool.start(timeout_s=WAIT_S)
        first = pool.workers[0].proc
        assert first is not None
        first.kill()
        self.wait_until(lambda: (folder / "2.txt").exists(), "a respawn")
        loading = pool.workers[0].proc
        assert loading is not None
        self.assertIsNot(loading, first)
        self.assertEqual(str(loading.pid), (folder / "2.txt").read_text(encoding="utf-8"))
        self.assertEqual(self.states(pool), {"worker-0": "loading"})
        pool.close()
        self.assertIsNotNone(loading.poll(), "the respawning worker outlived close()")
        left = [t.name for t in threading.enumerate() if t.name.startswith("voice-respawn")]
        self.assertEqual(left, [], "a respawn thread is still waiting for a load")

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
