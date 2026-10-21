# SPDX-License-Identifier: Apache-2.0
from __future__ import annotations

import os
import sys
import tempfile
import unittest
import wave
from array import array
from pathlib import Path

from shadowrunr_voice.engine import (
    BELOW_NORMAL_PRIORITY_CLASS,
    FACE_ARGS,
    EngineError,
    Render,
    build_args,
    creation_flags,
    narrow_safe,
    paths_for,
    read_pcm,
    scale_silence,
    spoken_entry,
    token_lines,
)

HERE = Path(__file__).resolve().parent
FAKE = [sys.executable, str(HERE / "fake_engine.py")]


def write_wav(path: Path, samples: list[int], rate: int = 24_000, channels: int = 1) -> None:
    data = array("h", samples)
    if sys.byteorder != "little":
        data.byteswap()
    with wave.open(str(path), "wb") as wav:
        wav.setnchannels(channels)
        wav.setsampwidth(2)
        wav.setframerate(rate)
        wav.writeframes(data.tobytes())


def engine_dir(root: Path, *, lexicon: bool = True) -> Path:
    """A Kokoro dir with empty model files: enough for the paths, never for a real engine."""
    kokoro = root / "kokoro"
    (kokoro / "espeak-ng-data").mkdir(parents=True)
    for name in ("model.onnx", "voices.bin", "tokens.txt"):
        (kokoro / name).write_bytes(b"")
    if lexicon:
        (kokoro / "lexicon-us-en.txt").write_text("", encoding="utf-8")
    return root


class NarrowSafe(unittest.TestCase):
    def test_off_windows_is_the_identity(self) -> None:
        self.assertEqual(narrow_safe("ř＂漢", None), "ř＂漢")

    def test_ascii_is_never_changed(self) -> None:
        text = 'I am Malakor. "Kneel," he said; 3.5 souls & a \\ path.'
        self.assertEqual(narrow_safe(text, 1252), text)

    def test_a_letter_loses_its_mark(self) -> None:
        # ř is not in 1252 and loses its caron; á is, and stays.
        self.assertEqual(narrow_safe("Dvořák", 1252), "Dvorák")

    def test_a_fullwidth_quote_is_its_plain_form(self) -> None:
        # Best fit would have made it a bare quote silently; folding makes it one list2cmdline
        # sees and quotes.
        self.assertEqual(narrow_safe("＂hi＂", 1252), '"hi"')

    def test_what_cannot_be_carried_is_a_space(self) -> None:
        self.assertEqual(narrow_safe("a漢b", 1252), "a b")

    def test_carried_non_ascii_stays(self) -> None:
        self.assertEqual(narrow_safe("café", 1252), "café")

    def test_spoken_entry_never_starts_with_a_dash(self) -> None:
        self.assertEqual(spoken_entry("--output-filename=x", None), " --output-filename=x")


class BuildArgs(unittest.TestCase):
    def test_order_with_the_text_last(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            paths = paths_for(engine_dir(Path(tmp)))
            out = Path(tmp) / "o.wav"
            args = build_args(paths, 3, out, "I am Malakor.", code_page=None)
            self.assertEqual(
                args,
                [
                    f"--kokoro-model={paths.model}",
                    f"--kokoro-voices={paths.voices}",
                    f"--kokoro-tokens={paths.tokens}",
                    f"--kokoro-data-dir={paths.espeak}",
                    f"--kokoro-lexicon={paths.lexicon}",
                    "--num-threads=2",
                    "--sid=3",
                    f"--output-filename={out}",
                    " I am Malakor.",
                ],
            )

    def test_no_lexicon_flag_without_the_file_and_extra_before_the_text(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            paths = paths_for(engine_dir(Path(tmp), lexicon=False))
            args = build_args(paths, 3, Path(tmp) / "o.wav", "Hi.", code_page=None, extra=FACE_ARGS)
            self.assertFalse(any(a.startswith("--kokoro-lexicon") for a in args))
            self.assertEqual(args[-3:], [*FACE_ARGS, " Hi."])

    def test_threads_are_the_pools_share(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            paths = paths_for(engine_dir(Path(tmp)))
            args = build_args(paths, 3, Path(tmp) / "o.wav", "Hi.", code_page=None, threads=5)
            self.assertIn("--num-threads=5", args)
            zero = build_args(paths, 3, Path(tmp) / "o.wav", "Hi.", code_page=None, threads=0)
            self.assertIn("--num-threads=1", zero)


class Priority(unittest.TestCase):
    def test_look_ahead_starts_below_normal_on_windows(self) -> None:
        urgent, ahead = creation_flags(True), creation_flags(False)
        if sys.platform == "win32":
            self.assertFalse(urgent & BELOW_NORMAL_PRIORITY_CLASS)
            self.assertTrue(ahead & BELOW_NORMAL_PRIORITY_CLASS)
        else:
            self.assertEqual((urgent, ahead), (0, 0))

    def test_promote_is_once_and_safe_before_the_process(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            render = Render(paths_for(engine_dir(Path(tmp))), 3, "Hi.", 5.0, urgent=False)
            render.promote()  # no process yet: only the flag
            self.assertTrue(render.urgent)
            render.promote()
            self.assertTrue(render.urgent)

    def test_missing_names_the_files_relative_to_the_engine_dir(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            missing = paths_for(Path(tmp)).missing()
            self.assertTrue(missing[0].startswith("bin/sherpa-onnx-offline-tts"))
            self.assertIn("kokoro/model.onnx", missing)
            self.assertTrue(all(tmp not in name for name in missing))

    def test_the_text_reaches_the_engine_as_one_argument_with_no_shell(self) -> None:
        hostile = 'He said "bow" & echo pwned; $(rm -rf x) | more %PATH% `id`'
        with tempfile.TemporaryDirectory() as tmp:
            root = engine_dir(Path(tmp))
            log = Path(tmp) / "log.txt"
            os.environ["FAKE_ENGINE_LOG"] = str(log)
            try:
                render = Render(paths_for(root), 3, hostile, 20.0, command=FAKE).start()
                rendered = render.wait()
            finally:
                del os.environ["FAKE_ENGINE_LOG"]
            self.assertIsNotNone(rendered)
            self.assertEqual(log.read_text(encoding="utf-8").strip(), hostile)


class ReadPcm(unittest.TestCase):
    def test_round_trip(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "t.wav"
            samples = [0, 1, -1, 32767, -32768, 1234]
            write_wav(path, samples)
            got = array("h")
            got.frombytes(read_pcm(path))
            if sys.byteorder != "little":
                got.byteswap()
            self.assertEqual(list(got), samples)

    def test_wrong_rate_or_channels_or_not_a_wav_is_an_engine_fault(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "t.wav"
            write_wav(path, [0, 0], rate=22_050)
            with self.assertRaises(EngineError):
                read_pcm(path)
            write_wav(path, [0, 0], channels=2)
            with self.assertRaises(EngineError):
                read_pcm(path)
            path.write_bytes(b"not a wav")
            with self.assertRaises(EngineError):
                read_pcm(path)


class Sherpa(unittest.TestCase):
    def test_token_lines(self) -> None:
        printed = "noise\n0 43 102 70 0\nElapsed seconds: 1.5\n 0 16 0 \n0 1 2\n"
        self.assertEqual(token_lines(printed), [[0, 43, 102, 70, 0], [0, 16, 0]])

    def test_scale_silence_without_a_pause_is_the_identity(self) -> None:
        pcm = array("h", [1000] * 100).tobytes()
        out, time_map = scale_silence(pcm, 24_000)
        self.assertEqual(out, pcm)
        self.assertEqual(time_map, [(0, 100, 0, 100)])

    def test_scale_silence_keeps_a_fifth_of_a_pause(self) -> None:
        rate = 1000  # a pause is then 200 samples or more
        samples = [5000] * 100 + [0] * 500 + [5000] * 100
        out, time_map = scale_silence(array("h", samples).tobytes(), rate)
        self.assertEqual(len(out) // 2, 100 + 100 + 100)
        self.assertEqual(time_map, [(0, 100, 0, 100), (100, 600, 100, 200), (600, 700, 200, 300)])

    def test_a_short_gap_is_not_a_pause(self) -> None:
        rate = 1000
        samples = [5000] * 10 + [0] * 150 + [5000] * 10
        out, _ = scale_silence(array("h", samples).tobytes(), rate)
        self.assertEqual(len(out) // 2, len(samples))


if __name__ == "__main__":
    unittest.main()
