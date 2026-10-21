# SPDX-License-Identifier: Apache-2.0
"""The lich chain over synthetic signals. It needs numpy, so it is skipped where numpy is not
importable (the standard-library interpreter, or ``python -S``); the warm venv runs it."""

from __future__ import annotations

import importlib.util
import unittest

HAVE_NUMPY = importlib.util.find_spec("numpy") is not None

if HAVE_NUMPY:
    import numpy as np

    from shadowrunr_voice import fx

RATE = 24_000


def pcm_of(x: np.ndarray) -> bytes:
    return np.round(np.clip(x, -1.0, 1.0) * 32767).astype("<i2").tobytes()


def samples_of(pcm: bytes) -> np.ndarray:
    return np.frombuffer(pcm, dtype="<i2")


def voiced(seconds: float, f0: float = 120.0, seed: int = 7) -> np.ndarray:
    """A buzzy vowel-ish tone: harmonics of ``f0`` with a little noise and a swell."""
    t = np.arange(int(seconds * RATE)) / RATE
    x = sum(np.sin(2 * np.pi * f0 * k * t) / k for k in range(1, 12))
    x = x + 0.05 * np.random.default_rng(seed).standard_normal(len(t))
    return 0.3 * x / np.max(np.abs(x)) * (0.6 + 0.4 * np.sin(2 * np.pi * 1.5 * t))


@unittest.skipUnless(HAVE_NUMPY, "the lich chain needs numpy")
class Length(unittest.TestCase):
    def test_every_preset_keeps_the_sample_count(self) -> None:
        source = pcm_of(voiced(3.0))
        for n in (0, 1, 100, fx.N_FFT - 1, fx.N_FFT + 1, RATE + 7, 3 * RATE):
            pcm = source[: 2 * n]
            for preset in fx.PRESETS:
                with self.subTest(n=n, preset=preset):
                    self.assertEqual(len(fx.apply(pcm, preset)), 2 * n)

    def test_an_odd_trailing_byte_is_dropped(self) -> None:
        pcm = pcm_of(voiced(0.2)) + b"\x01"
        self.assertEqual(len(fx.apply(pcm, "light")), len(pcm) - 1)


@unittest.skipUnless(HAVE_NUMPY, "the lich chain needs numpy")
class Determinism(unittest.TestCase):
    def test_the_same_input_gives_the_same_bytes(self) -> None:
        pcm = pcm_of(voiced(1.3))
        for preset in fx.PRESETS:
            with self.subTest(preset=preset):
                self.assertEqual(fx.apply(pcm, preset), fx.apply(pcm, preset))

    def test_the_crypt_is_generated_the_same_every_time(self) -> None:
        a = fx.crypt_impulse.__wrapped__(1.2)
        b = fx.crypt_impulse.__wrapped__(1.2)
        self.assertTrue(np.array_equal(a, b))
        self.assertAlmostEqual(float(np.sum(a**2)), 1.0, places=9)
        self.assertEqual(len(a), int(1.2 * RATE))
        late, early = a[int(0.9 * RATE) :], a[: int(0.3 * RATE)]
        self.assertLess(np.sqrt(np.mean(late**2)), np.sqrt(np.mean(early**2)) / 10)


@unittest.skipUnless(HAVE_NUMPY, "the lich chain needs numpy")
class Limiter(unittest.TestCase):
    def test_a_full_scale_input_never_clips(self) -> None:
        t = np.arange(2 * RATE) / RATE
        square = np.sign(np.sin(2 * np.pi * 90 * t))
        noise = np.clip(np.random.default_rng(3).standard_normal(2 * RATE), -1, 1)
        for name, x in (("square", square), ("noise", noise), ("voice", voiced(2.0) * 3)):
            for preset in fx.PRESETS:
                with self.subTest(signal=name, preset=preset):
                    out = samples_of(fx.apply(pcm_of(x), preset)).astype(np.int32)
                    # The ceiling in PCM16 (31948) is under full scale (32767).
                    self.assertLessEqual(int(np.max(np.abs(out))), round(fx.CEILING * 32767))

    def test_the_limiter_gain_alone_stays_under_the_ceiling(self) -> None:
        # Hot impulses and steps, far over the ceiling: the gain holds them, not the final clip.
        x = np.zeros(RATE)
        x[100::997] = 9.0
        x[5000:5200] = -6.0
        x[RATE - 1] = 4.0
        for ceiling in (fx.CEILING, 0.5):
            with self.subTest(ceiling=ceiling):
                gained = x * fx.limiter_gain(x, RATE, ceiling)
                self.assertLessEqual(float(np.max(np.abs(gained))), ceiling + 1e-9)

    def test_quiet_audio_passes_the_limiter_untouched(self) -> None:
        x = 0.25 * np.sin(np.linspace(0, 200, RATE))
        self.assertTrue(np.allclose(fx._limit(x, RATE), x))


@unittest.skipUnless(HAVE_NUMPY, "the lich chain needs numpy")
class Pitch(unittest.TestCase):
    def test_a_tone_moves_down_by_the_presets_semitones(self) -> None:
        t = np.arange(2 * RATE) / RATE
        x = 0.3 * np.sin(2 * np.pi * 200 * t)
        for name, preset in fx.PRESETS.items():
            with self.subTest(preset=name):
                y = fx.process(x, name)[RATE // 2 : RATE // 2 + RATE]
                peak = int(np.argmax(np.abs(np.fft.rfft(y))))  # 1 Hz bins over 1 s
                want = 200 * 2 ** (preset.semitones / 12)
                self.assertLess(abs(peak - want), 3.0)


if __name__ == "__main__":
    unittest.main()
