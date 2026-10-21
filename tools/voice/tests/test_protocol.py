# SPDX-License-Identifier: Apache-2.0
"""The protocol over a fake engine (``fake_engine.py``): no engine, no models, no numpy."""

from __future__ import annotations

import base64
import io
import json
import os
import subprocess
import sys
import tempfile
import threading
import time
import unittest
from pathlib import Path
from typing import Any
from unittest import mock

from shadowrunr_voice.__main__ import run
from shadowrunr_voice.sidecar import Faces, Out, Settings, Sidecar, hello

from .test_engine import FAKE, engine_dir

TOOLS = Path(__file__).resolve().parent.parent
WAIT_S = 20.0


class Sink:
    """What the sidecar sent, and a way to wait for it."""

    def __init__(self) -> None:
        self.messages: list[dict[str, Any]] = []
        self.cond = threading.Condition()

    def send(self, message: dict[str, Any]) -> None:
        json.dumps(message)  # every message must be JSON as it is
        with self.cond:
            self.messages.append(message)
            self.cond.notify_all()

    def wait_for(self, done: Any, timeout: float = WAIT_S) -> list[dict[str, Any]]:
        with self.cond:
            self.cond.wait_for(lambda: done(self.messages), timeout)
            return list(self.messages)


class FakeDurations:
    """Four frames per word token, none for the pads: what ``fake_engine.py`` writes."""

    def frames(self, tokens: list[int], sid: int) -> list[int]:
        return [0 if t == 0 else (0 if t == 50 else 4) for t in tokens]


class Protocol(unittest.TestCase):
    def setUp(self) -> None:
        self.tmp = tempfile.TemporaryDirectory()
        self.root = engine_dir(Path(self.tmp.name))
        self.log = Path(self.tmp.name) / "engine-log.txt"
        os.environ["FAKE_ENGINE_LOG"] = str(self.log)
        self.sink = Sink()
        self.sidecars: list[Sidecar] = []

    def tearDown(self) -> None:
        for sidecar in self.sidecars:
            sidecar.close()
        del os.environ["FAKE_ENGINE_LOG"]
        self.tmp.cleanup()

    def sidecar(
        self, *, timeout_s: float = 20.0, faces: Faces | None = None, workers: int = 3
    ) -> Sidecar:
        settings = Settings(
            engine_dir=self.root, timeout_s=timeout_s, command=FAKE, workers=workers
        )
        sidecar = Sidecar(settings, self.sink.send, faces=faces)
        self.sidecars.append(sidecar)
        return sidecar

    def say(self, sidecar: Sidecar, ident: Any, text: str) -> None:
        sidecar.feed(json.dumps({"type": "say", "id": ident, "text": text}))

    def test_hello_first_with_the_voice_and_no_visemes_without_a_model(self) -> None:
        settings = Settings(engine_dir=self.root, command=FAKE)
        message, dead, faces, warm = hello(settings)
        self.assertIsNone(dead)
        self.assertIsNone(faces)
        self.assertIsNone(warm)
        self.assertEqual(
            {k: v for k, v in message.items() if k != "reason"},
            {
                "type": "hello",
                "protocol": 1,
                "voice": "af_heart",
                "sample_rate": 24_000,
                "visemes": False,
            },
        )
        self.assertIn("durations", message["reason"])

    def test_a_missing_durations_model_or_missing_packages_is_a_reason(self) -> None:
        missing = Settings(engine_dir=self.root, command=FAKE, durations=self.root / "nope.onnx")
        self.assertIn("missing", hello(missing)[0]["reason"])
        present = self.root / "durations.onnx"
        present.write_bytes(b"")
        settings = Settings(engine_dir=self.root, command=FAKE, durations=present)
        with mock.patch.dict(sys.modules, {"numpy": None, "onnxruntime": None}):
            message, dead, _, _ = hello(settings)
        self.assertIsNone(dead)
        self.assertFalse(message["visemes"])
        self.assertIn("not installed", message["reason"])

    def test_audio_per_sentence_in_order(self) -> None:
        sidecar = self.sidecar()
        self.say(sidecar, "line-1", "I am Malakor. Kneel before the crypt. Your souls are mine.")
        got = self.sink.wait_for(lambda m: any(x.get("last") for x in m))
        self.assertEqual([m["type"] for m in got], ["audio"] * 3)
        self.assertEqual([m["part"] for m in got], [0, 1, 2])
        self.assertEqual([m["last"] for m in got], [False, False, True])
        self.assertEqual(
            [m["text"] for m in got],
            ["I am Malakor.", "Kneel before the crypt.", "Your souls are mine."],
        )
        for m in got:
            self.assertEqual(m["id"], "line-1")
            self.assertEqual(m["sample_rate"], 24_000)
            pcm = base64.b64decode(m["pcm"])
            self.assertGreater(len(pcm), 0)
            self.assertEqual(len(pcm) % 2, 0)
            self.assertNotIn("visemes", m)  # omitted, never null
        # The pool starts them together; which process logs first is the OS's choice.
        self.assertEqual(
            sorted(self.log.read_text(encoding="utf-8").splitlines()),
            ["I am Malakor.", "Kneel before the crypt.", "Your souls are mine."],
        )

    def test_lines_are_spoken_in_arrival_order(self) -> None:
        sidecar = self.sidecar()
        self.say(sidecar, 1, "First line.")
        self.say(sidecar, 2, "Second line.")
        got = self.sink.wait_for(lambda m: len(m) == 2)
        self.assertEqual([(m["id"], m["part"], m["last"]) for m in got], [(1, 0, True), (2, 0, True)])

    def test_a_failed_sentence_is_an_error_and_ends_the_line(self) -> None:
        sidecar = self.sidecar()
        self.say(sidecar, "x", "This one is fine. This one will FAIL now. Never said at all.")
        got = self.sink.wait_for(lambda m: any(x["type"] == "error" for x in m))
        time.sleep(0.3)
        got = self.sink.messages
        self.assertEqual([(m["type"], m["part"]) for m in got], [("audio", 0), ("error", 1)])
        self.assertEqual(set(got[1]), {"type", "id", "part", "reason"})
        self.assertIn("exited with 3", got[1]["reason"])

    def test_a_render_past_the_timeout_is_an_error(self) -> None:
        sidecar = self.sidecar(timeout_s=0.5)
        self.say(sidecar, "x", "This is SLOW to say.")
        got = self.sink.wait_for(lambda m: len(m) == 1)
        self.assertEqual(got[0]["type"], "error")
        self.assertEqual(got[0]["part"], 0)
        self.assertIn("timed out after 500 ms", got[0]["reason"])

    def test_cancel_kills_the_render_and_skips_queued_lines(self) -> None:
        sidecar = self.sidecar(workers=1)  # one slot: "queued" really waits
        self.say(sidecar, "slow", "This is SLOW to say. And more after it.")
        self.say(sidecar, "queued", "Never spoken.")
        time.sleep(0.5)  # the slow render is running
        began = time.monotonic()
        sidecar.feed(json.dumps({"type": "cancel", "ids": ["queued", "slow"]}))
        self.say(sidecar, "after", "Spoken after the cancel.")
        got = self.sink.wait_for(lambda m: len(m) == 1)
        self.assertLess(time.monotonic() - began, 10)
        time.sleep(0.3)
        self.assertEqual([(m["type"], m["id"]) for m in self.sink.messages], [("audio", "after")])
        self.assertNotIn("Never spoken.", self.log.read_text(encoding="utf-8"))
        self.assertEqual(got[0]["text"], "Spoken after the cancel.")

    def test_cancel_of_an_unknown_id_changes_nothing(self) -> None:
        sidecar = self.sidecar()
        sidecar.feed(json.dumps({"type": "cancel", "ids": ["nobody", 7]}))
        self.say(sidecar, "a", "Still here.")
        self.assertEqual(len(self.sink.wait_for(lambda m: len(m) == 1)), 1)

    def test_a_line_with_no_words_is_an_error(self) -> None:
        sidecar = self.sidecar()
        self.say(sidecar, "e", "   ")
        got = self.sink.wait_for(lambda m: len(m) == 1)
        self.assertEqual((got[0]["type"], got[0]["part"]), ("error", 0))

    def test_junk_input_is_ignored(self) -> None:
        sidecar = self.sidecar()
        sidecar.feed("not json")
        sidecar.feed("[1, 2]")
        sidecar.feed(json.dumps({"type": "say", "text": "no id"}))
        sidecar.feed(json.dumps({"type": "mystery"}))
        self.say(sidecar, "ok", "Fine.")
        got = self.sink.wait_for(lambda m: len(m) == 1)
        self.assertEqual(got[0]["id"], "ok")

    def test_visemes_ride_with_their_sentence(self) -> None:
        faces = Faces(FakeDurations(), {16: "ə", 50: " "}, 3)  # type: ignore[arg-type]
        sidecar = self.sidecar(faces=faces)
        self.say(sidecar, "v", "Words for a face.")
        got = self.sink.wait_for(lambda m: len(m) == 1)
        audio = got[0]
        samples = len(base64.b64decode(audio["pcm"])) // 2
        vis = audio["visemes"]
        self.assertTrue(vis)
        self.assertEqual(vis[0][1], 0)
        self.assertEqual(vis[-1][2], round(samples * 1000 / 24_000))
        for v, start, end in vis:
            self.assertIsInstance(v, str)
            self.assertLess(start, end)

    def test_missing_engine_is_a_reasoned_hello_and_errors_for_every_say(self) -> None:
        with tempfile.TemporaryDirectory() as empty:
            settings = Settings(engine_dir=Path(empty))
            stream = io.StringIO()
            inbox = [
                json.dumps({"type": "say", "id": "a", "text": "Hello there."}) + "\n",
                json.dumps({"type": "say", "id": "b", "text": "And again."}) + "\n",
            ]
            done = threading.Event()

            def lines() -> Any:
                yield from inbox
                done.wait(2)  # let the worker answer before stdin "closes"

            thread = threading.Thread(target=run, args=(settings, lines(), Out(stream)))
            thread.start()
            deadline = time.monotonic() + WAIT_S
            while stream.getvalue().count("\n") < 3 and time.monotonic() < deadline:
                time.sleep(0.02)
            done.set()
            thread.join(WAIT_S)
            out = [json.loads(line) for line in stream.getvalue().splitlines()]
        self.assertEqual(out[0]["type"], "hello")
        self.assertFalse(out[0]["visemes"])
        self.assertIn("sherpa-onnx-offline-tts", out[0]["reason"])
        self.assertNotIn(empty, out[0]["reason"])
        self.assertEqual(
            [(m["type"], m["id"], m["part"]) for m in out[1:]], [("error", "a", 0), ("error", "b", 0)]
        )
        self.assertEqual(out[1]["reason"], out[0]["reason"])


class Process(unittest.TestCase):
    """The real ``python -m shadowrunr_voice``: stdout is the protocol only, and it exits when
    stdin closes."""

    def test_stdout_framing_and_exit_on_stdin_close(self) -> None:
        with tempfile.TemporaryDirectory() as empty:
            proc = subprocess.Popen(
                [sys.executable, "-m", "shadowrunr_voice", "--engine-dir", empty, "--sid", "3",
                 "--voice", "af_heart", "--render-timeout-ms", "5000"],
                cwd=TOOLS,
                stdin=subprocess.PIPE,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
            )  # fmt: skip
            assert proc.stdin is not None and proc.stdout is not None
            first = json.loads(proc.stdout.readline())
            proc.stdin.write(b'{"type":"say","id":"a","text":"Hi there."}\n')
            proc.stdin.flush()
            second = json.loads(proc.stdout.readline())
            proc.stdin.close()
            code = proc.wait(timeout=WAIT_S)
            rest = proc.stdout.read()
            proc.stdout.close()
            assert proc.stderr is not None
            proc.stderr.close()
        self.assertEqual(code, 0)
        self.assertEqual(first["type"], "hello")
        self.assertEqual(first["protocol"], 1)
        self.assertEqual((second["type"], second["id"], second["part"]), ("error", "a", 0))
        self.assertEqual(rest, b"")


if __name__ == "__main__":
    unittest.main()
