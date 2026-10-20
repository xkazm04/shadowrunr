# SPDX-License-Identifier: Apache-2.0
"""The real engine, opt in: skipped unless ``SHADOWRUNR_VOICE_ENGINE_DIR`` names the engine dir
(``bin/`` and ``kokoro/``). With ``SHADOWRUNR_VOICE_DURATIONS`` naming Kokoro's durations
subgraph, and numpy and onnxruntime in this interpreter, the mouth shapes are checked too."""

from __future__ import annotations

import base64
import os
import time
import unittest
from pathlib import Path

from shadowrunr_voice.sidecar import Settings, Sidecar, hello

from .test_protocol import Sink

ENGINE_DIR = os.environ.get("SHADOWRUNR_VOICE_ENGINE_DIR")
DURATIONS = os.environ.get("SHADOWRUNR_VOICE_DURATIONS")


@unittest.skipUnless(ENGINE_DIR, "set SHADOWRUNR_VOICE_ENGINE_DIR to run the real engine")
class RealEngine(unittest.TestCase):
    def test_i_am_malakor(self) -> None:
        assert ENGINE_DIR is not None
        settings = Settings(
            engine_dir=Path(ENGINE_DIR),
            timeout_s=90.0,
            durations=Path(DURATIONS) if DURATIONS else None,
        )
        message, dead, faces = hello(settings)
        self.assertIsNone(dead, message.get("reason"))
        self.assertEqual(message["sample_rate"], 24_000)
        if DURATIONS:
            self.assertTrue(message["visemes"], message.get("reason"))
        sink = Sink()
        sidecar = Sidecar(settings, sink.send, faces=faces)
        try:
            began = time.monotonic()
            sidecar.feed('{"type":"say","id":"smoke","text":"I am Malakor."}')
            got = sink.wait_for(lambda m: len(m) == 1, timeout=120)
            print(f"\nsmoke: first audio after {round((time.monotonic() - began) * 1000)} ms")
        finally:
            sidecar.close()
        self.assertEqual(len(got), 1, got)
        audio = got[0]
        self.assertEqual((audio["type"], audio["part"], audio["last"]), ("audio", 0, True))
        self.assertEqual(audio["sample_rate"], 24_000)
        pcm = base64.b64decode(audio["pcm"])
        self.assertGreater(len(pcm), 24_000 // 2 * 2, "under half a second of audio")
        duration_ms = len(pcm) // 2 * 1000 / 24_000
        if DURATIONS:
            vis = audio.get("visemes")
            self.assertTrue(vis, "no visemes")
            self.assertGreaterEqual(vis[0][1], 0)
            self.assertLessEqual(vis[-1][2], round(duration_ms))
            self.assertTrue(any(v != "sil" for v, _, _ in vis))
            self.assertIn("PP", [v for v, _, _ in vis])  # the m of "am" and "Malakor"


if __name__ == "__main__":
    unittest.main()
