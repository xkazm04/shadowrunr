# SPDX-License-Identifier: Apache-2.0
"""The voice the sidecar speaks: its voices file (``--voices``) and its FX chain (``--fx``), over
the fake engine. No engine, no models; the FX chain itself runs only where numpy is importable."""

from __future__ import annotations

import base64
import importlib.util
import json
import sys
import tempfile
import unittest
from pathlib import Path
from typing import Any
from unittest import mock

from shadowrunr_voice.__main__ import parse
from shadowrunr_voice.engine import SPEAKER_BYTES, build_args, paths_for, voices_problem
from shadowrunr_voice.sidecar import Faces, Settings, Sidecar, hello, voice_problem
from shadowrunr_voice.warm import worker_argv

from .test_engine import FAKE, engine_dir
from .test_protocol import FakeDurations, Sink

HAVE_NUMPY = importlib.util.find_spec("numpy") is not None
#: The test's stock voices file: two speakers, so a sid past 1 is out of range.
STOCK_SPEAKERS = 2


class Voices(unittest.TestCase):
    def setUp(self) -> None:
        self.tmp = tempfile.TemporaryDirectory()
        base = Path(self.tmp.name)
        self.root = engine_dir(base / "engine")
        self.stock = self.root / "kokoro" / "voices.bin"
        self.stock.write_bytes(bytes(STOCK_SPEAKERS * SPEAKER_BYTES))
        self.blend = base / "voices" / "malakor-voices.bin"
        self.blend.parent.mkdir()
        self.blend.write_bytes(bytes(STOCK_SPEAKERS * SPEAKER_BYTES))
        self.sink = Sink()
        self.sidecars: list[Sidecar] = []

    def tearDown(self) -> None:
        for sidecar in self.sidecars:
            sidecar.close()
        self.tmp.cleanup()

    def settings(self, **kw: Any) -> Settings:
        kw.setdefault("voices", self.blend)
        kw.setdefault("sid", 1)
        return Settings(engine_dir=self.root, command=FAKE, **kw)

    def speak(self, settings: Settings, text: str, **kw: Any) -> dict[str, Any]:
        sidecar = Sidecar(settings, self.sink.send, **kw)
        self.sidecars.append(sidecar)
        before = len(self.sink.messages)
        sidecar.feed(json.dumps({"type": "say", "id": f"l{before}", "text": text}))
        return self.sink.wait_for(lambda m: len(m) > before)[before]

    # -- the voices file -------------------------------------------------------------------------

    def test_the_hello_names_the_voices_file_the_sid_and_the_fx_never_a_path(self) -> None:
        message, dead, _, _ = hello(self.settings(voice="malakor-a"))
        self.assertIsNone(dead)
        self.assertEqual(
            (message["voice"], message["voices"], message["sid"], message["fx"]),
            ("malakor-a", "malakor-voices.bin", 1, "none"),
        )
        self.assertNotIn(self.tmp.name, json.dumps(message))
        stock, _, _, _ = hello(Settings(engine_dir=self.root, command=FAKE))
        self.assertEqual((stock["voices"], stock["sid"]), ("voices.bin", 3))

    def test_every_reader_takes_the_one_voices_file(self) -> None:
        settings = self.settings()
        self.assertEqual(settings.paths.voices, self.blend)
        args = build_args(settings.paths, 1, Path("out.wav"), "Kneel.", code_page=None)
        self.assertIn(f"--kokoro-voices={self.blend}", args)
        argv = worker_argv(self.root, 1, 4, True, self.blend)
        self.assertEqual(argv[argv.index("--voices") + 1], str(self.blend))
        self.assertNotIn("--voices", worker_argv(self.root, 1, 4, True))
        # The model's other files still come from the engine dir.
        self.assertEqual(settings.paths.model, self.root / "kokoro" / "model.onnx")

    def test_a_missing_voices_file_is_dead_and_never_the_stock_voice(self) -> None:
        self.blend.unlink()
        settings = self.settings()
        message, dead, faces, warm = hello(settings)
        self.assertEqual(dead, "the voices file malakor-voices.bin is missing")
        self.assertEqual(message["reason"], dead)
        self.assertIsNone(faces)
        self.assertIsNone(warm)
        got = self.speak(settings, "Hello there.", dead=dead)
        self.assertEqual((got["type"], got["part"], got["reason"]), ("error", 0, dead))

    def test_a_voices_file_of_the_wrong_size_is_dead(self) -> None:
        self.blend.write_bytes(bytes(STOCK_SPEAKERS * SPEAKER_BYTES + 4))
        dead = voice_problem(self.settings())
        assert dead is not None
        self.assertIn("malakor-voices.bin is", dead)
        self.assertIn(str(STOCK_SPEAKERS * SPEAKER_BYTES), dead)
        self.assertNotIn(self.tmp.name, dead)

    def test_without_the_stock_file_the_size_is_the_model_s_53_speakers(self) -> None:
        self.stock.unlink()
        paths = paths_for(self.root, self.blend)
        self.assertIn(str(53 * SPEAKER_BYTES), voices_problem(paths, 1) or "")
        self.blend.write_bytes(bytes(53 * SPEAKER_BYTES))
        self.assertIsNone(voices_problem(paths, 52))

    def test_a_sid_past_the_file_s_speakers_is_dead(self) -> None:
        dead = voice_problem(self.settings(sid=STOCK_SPEAKERS))
        self.assertEqual(
            dead, "sid 2 is not one of the 2 speakers in the voices file malakor-voices.bin"
        )

    def test_the_stock_voices_are_checked_as_before(self) -> None:
        self.assertIsNone(voice_problem(Settings(engine_dir=self.root, command=FAKE)))
        self.assertFalse(paths_for(self.root).own_voices)
        self.assertFalse(paths_for(self.root, self.stock).own_voices)

    # -- the FX chain ----------------------------------------------------------------------------

    def test_fx_without_numpy_is_dead_not_dry(self) -> None:
        with mock.patch.dict(sys.modules, {"numpy": None}):
            sys.modules.pop("shadowrunr_voice.fx", None)
            message, dead, _, _ = hello(self.settings(fx="light"))
        self.assertEqual(dead, "fx light needs numpy, which is not installed")
        self.assertEqual(message["fx"], "light")
        self.assertEqual(voice_problem(self.settings(fx="loud")), "fx loud is not a preset "
                         "(none, light, heavy)")  # fmt: skip

    def test_the_chain_runs_after_the_mouth_shapes_and_keeps_the_length(self) -> None:
        faces = Faces(FakeDurations(), {16: "ə", 50: " "}, 1)  # type: ignore[arg-type]
        seen: list[bytes] = []

        def silence(pcm: bytes) -> bytes:
            seen.append(pcm)
            return bytes(len(pcm))

        dry = self.speak(self.settings(), "Words for a face.", faces=faces)
        wet = self.speak(self.settings(fx="light"), "Words for a face.", faces=faces, wet=silence)
        self.assertEqual(len(seen), 1)
        self.assertEqual(base64.b64decode(dry["pcm"]), seen[0], "the chain gets the dry audio")
        self.assertEqual(base64.b64decode(wet["pcm"]), bytes(len(seen[0])))
        self.assertEqual(wet["visemes"], dry["visemes"], "the mouth comes from the dry audio")

    @unittest.skipUnless(HAVE_NUMPY, "the lich chain needs numpy")
    def test_the_real_chain_changes_the_audio_not_its_length(self) -> None:
        dry = base64.b64decode(self.speak(self.settings(), "Kneel before me.")["pcm"])
        wet = base64.b64decode(self.speak(self.settings(fx="light"), "Kneel before me.")["pcm"])
        self.assertEqual(len(wet), len(dry))
        self.assertNotEqual(wet, dry)

    # -- the command line ------------------------------------------------------------------------

    def test_the_flags(self) -> None:
        s = parse(["--engine-dir", "E", "--voices", "v.bin", "--sid", "52", "--fx", "light"])
        self.assertEqual((s.voices, s.sid, s.fx), (Path("v.bin"), 52, "light"))
        bare = parse(["--engine-dir", "E"])
        self.assertEqual((bare.voices, bare.fx), (None, "none"))
        with mock.patch("sys.stderr"), self.assertRaises(SystemExit):
            parse(["--engine-dir", "E", "--fx", "loud"])


if __name__ == "__main__":
    unittest.main()
