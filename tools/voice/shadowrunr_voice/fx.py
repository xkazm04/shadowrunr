# SPDX-License-Identifier: Apache-2.0
"""The lich chain: a deterministic numpy-only effect over Kokoro's 24 kHz mono PCM16.

The output has exactly as many samples as the input, so a sentence's mouth timeline (worked out
on the dry audio) still holds for the wet one. The chain, in order:

1. **Pitch and formants, one STFT pass.** Each frame's magnitude is split into a smooth spectral
   envelope (cepstral lifter) and its fine structure (the harmonics). The fine structure is
   resampled along frequency by the pitch ratio, the envelope by the formant ratio, and each
   output bin takes the phase-vocoder phase advance of its source bin, scaled by the pitch
   ratio. Time is untouched, so the length is too. A high-pass and a presence lift ride along
   in the same spectrum.
2. **Grit.** ``tanh`` saturation, mixed under the dry signal.
3. **Doubling.** A short delayed copy, for a hollow chest (``heavy`` only).
4. **Crypt.** Convolution with a generated impulse: a few early reflections and a decaying
   noise tail that darkens as it decays, from a fixed seed. Nothing is downloaded. The tail past
   the sentence's last sample is dropped.
5. **Compressor, then limiter.** A block RMS compressor with attack and release, make-up gain,
   then a look-ahead limiter whose gain is a moving average of a running minimum, which can never
   rise above the gain a sample needs, so nothing reaches the ceiling, let alone clips.

``python -m shadowrunr_voice.fx --preset heavy in.wav out.wav`` runs it on a WAV.
"""

from __future__ import annotations

import argparse
import functools
import sys
import wave
from collections.abc import Sequence
from dataclasses import dataclass
from pathlib import Path

import numpy as np

__all__ = ["PRESETS", "Preset", "apply", "crypt_impulse", "limiter_gain", "process"]

SAMPLE_RATE = 24_000
N_FFT = 1024
HOP = 256
#: Cepstral coefficients kept for the envelope: under the shortest pitch period (~2.5 ms).
LIFTER = 40
#: The limiter's ceiling on the ``[-1, 1]`` scale (about -0.2 dBFS).
CEILING = 0.975
LOOKAHEAD_S = 0.002
BLOCK_S = 0.005
IR_SEED = 0x11C4


@dataclass(frozen=True)
class Preset:
    semitones: float  # pitch shift, negative is down
    formant: float  # formant ratio, under 1 is a longer, larger throat
    highpass_hz: float
    presence_db: float  # lift around 2.5-5 kHz
    grit: float  # saturation mix, 0..1
    drive: float
    double_mix: float  # 0 is off
    double_ms: float
    reverb_mix: float
    rt60_s: float
    comp_threshold_db: float
    comp_ratio: float
    makeup_db: float


PRESETS: dict[str, Preset] = {
    "light": Preset(
        semitones=-2.5, formant=0.92, highpass_hz=70.0, presence_db=2.0, grit=0.15, drive=2.5,
        double_mix=0.0, double_ms=0.0, reverb_mix=0.14, rt60_s=0.9, comp_threshold_db=-20.0,
        comp_ratio=3.0, makeup_db=4.0,
    ),
    "heavy": Preset(
        semitones=-4.0, formant=0.86, highpass_hz=80.0, presence_db=3.0, grit=0.35, drive=4.0,
        double_mix=0.22, double_ms=13.0, reverb_mix=0.26, rt60_s=1.8, comp_threshold_db=-22.0,
        comp_ratio=4.0, makeup_db=5.0,
    ),
}  # fmt: skip


# --- the spectrum -------------------------------------------------------------------------------


@functools.lru_cache(maxsize=4)
def _window(n: int) -> np.ndarray:
    return np.hanning(n + 1)[:-1].astype(np.float64)  # periodic Hann


def _stft(x: np.ndarray) -> np.ndarray:
    frames = 1 + (len(x) - N_FFT) // HOP
    idx = np.arange(N_FFT)[None, :] + HOP * np.arange(frames)[:, None]
    return np.fft.rfft(x[idx] * _window(N_FFT), axis=1)


def _istft(spec: np.ndarray, length: int) -> np.ndarray:
    frames = np.fft.irfft(spec, n=N_FFT, axis=1) * _window(N_FFT)
    out = np.zeros(length)
    norm = np.zeros(length)
    w2 = _window(N_FFT) ** 2
    for i, frame in enumerate(frames):
        at = i * HOP
        out[at : at + N_FFT] += frame
        norm[at : at + N_FFT] += w2
    return out / np.maximum(norm, 1e-8)


def _resample_bins(a: np.ndarray, ratio: float) -> np.ndarray:
    """Each output bin ``k`` reads bin ``k / ratio`` of ``a`` (linear); past the top it is 0."""
    bins = a.shape[1]
    src = np.arange(bins) / ratio
    i0 = np.minimum(np.floor(src).astype(np.int64), bins - 1)
    i1 = np.minimum(i0 + 1, bins - 1)
    frac = src - i0
    out = a[:, i0] * (1.0 - frac) + a[:, i1] * frac
    out[:, src > bins - 1] = 0.0
    return out


def _shift(x: np.ndarray, p: Preset, sr: int) -> np.ndarray:
    n = len(x)
    pad = N_FFT
    padded = np.concatenate([np.zeros(pad), x, np.zeros(pad + HOP)])
    spec = _stft(padded)
    mag = np.abs(spec)
    phase = np.angle(spec)
    # The envelope: low quefrencies of the log spectrum.
    ceps = np.fft.irfft(np.log(mag + 1e-9), n=N_FFT, axis=1)
    ceps[:, LIFTER : N_FFT - LIFTER + 1] = 0.0
    env = np.exp(np.fft.rfft(ceps, axis=1).real)
    fine = mag / env
    # The true phase advance per hop of each bin.
    bins = np.arange(mag.shape[1])
    omega = 2.0 * np.pi * HOP * bins / N_FFT
    dphi = np.diff(phase, axis=0, prepend=phase[:1]) - omega
    dphi = (dphi + np.pi) % (2.0 * np.pi) - np.pi
    advance = omega + dphi
    ratio = 2.0 ** (p.semitones / 12.0)
    new_mag = _resample_bins(fine, ratio) * _resample_bins(env, p.formant)
    new_adv = _resample_bins(advance, ratio) * ratio
    new_phase = np.cumsum(new_adv, axis=0)
    hz = bins * sr / N_FFT
    shape = np.where(hz < p.highpass_hz, (hz / p.highpass_hz) ** 2, 1.0)
    presence = np.exp(-0.5 * ((hz - 3500.0) / 1000.0) ** 2)
    shape = shape * 10.0 ** (p.presence_db * presence / 20.0)
    out = _istft(new_mag * shape * np.exp(1j * new_phase), len(padded))
    return out[pad : pad + n]


# --- the rest of the chain ----------------------------------------------------------------------


@functools.lru_cache(maxsize=8)
def crypt_impulse(rt60_s: float, sr: int = SAMPLE_RATE, seed: int = IR_SEED) -> np.ndarray:
    """A stone room's impulse, made, not recorded: early reflections, then noise decaying by
    60 dB over ``rt60_s`` and losing its top as it goes. The same arguments give the same IR."""
    rng = np.random.default_rng(seed)
    length = int(rt60_s * sr)
    t = np.arange(length) / sr
    noise = rng.standard_normal(length)
    # Darkening: a running mean whose width grows with time (via a cumulative sum).
    width = 1 + (t / rt60_s * 24).astype(np.int64)
    csum = np.concatenate([[0.0], np.cumsum(noise)])
    lo = np.maximum(np.arange(length) - width + 1, 0)
    dark = (csum[np.arange(length) + 1] - csum[lo]) / (np.arange(length) + 1 - lo)
    tail = dark * np.sqrt(width) * 10.0 ** (-3.0 * t / rt60_s)
    predelay = int(0.018 * sr)
    tail[:predelay] = 0.0
    for ms, gain in ((11.0, 0.5), (17.0, -0.35), (29.0, 0.3), (41.0, -0.22), (53.0, 0.18)):
        tail[int(ms * sr / 1000)] += gain * 4.0
    return tail / np.sqrt(np.sum(tail**2))


def _convolve(x: np.ndarray, ir: np.ndarray) -> np.ndarray:
    size = 1 << int(np.ceil(np.log2(len(x) + len(ir))))
    y = np.fft.irfft(np.fft.rfft(x, size) * np.fft.rfft(ir, size), size)
    return y[: len(x)]


def _compress(x: np.ndarray, p: Preset, sr: int) -> np.ndarray:
    block = max(int(BLOCK_S * sr), 1)
    blocks = -(-len(x) // block)
    padded = np.zeros(blocks * block)
    padded[: len(x)] = x
    rms = np.sqrt(np.mean(padded.reshape(blocks, block) ** 2, axis=1) + 1e-12)
    level = 20.0 * np.log10(rms)
    over = np.maximum(level - p.comp_threshold_db, 0.0)
    target = -over * (1.0 - 1.0 / p.comp_ratio)
    attack = np.exp(-BLOCK_S / 0.005)
    release = np.exp(-BLOCK_S / 0.120)
    gain_db = np.empty(blocks)
    g = 0.0
    for i, want in enumerate(target):
        coeff = attack if want < g else release
        g = coeff * g + (1.0 - coeff) * want
        gain_db[i] = g
    centres = (np.arange(blocks) + 0.5) * block
    per_sample = np.interp(np.arange(len(x)), centres, gain_db)
    return x * 10.0 ** ((per_sample + p.makeup_db) / 20.0)


def limiter_gain(x: np.ndarray, sr: int, ceiling: float = CEILING) -> np.ndarray:
    """The look-ahead limiter's gain per sample: at most what each sample needs to stay under
    ``ceiling``, reached over :data:`LOOKAHEAD_S` before a peak."""
    look = max(int(LOOKAHEAD_S * sr), 1)
    need = np.minimum(1.0, ceiling / np.maximum(np.abs(x), 1e-12))
    ahead = np.concatenate([need, np.ones(look - 1)])
    floor = np.lib.stride_tricks.sliding_window_view(ahead, look).min(axis=1)
    # A moving average over the last `look` running minima. Each of them covers this sample (the
    # first ones stand in for those before the start), so the average never exceeds the gain
    # this sample needs.
    lead = np.concatenate([np.full(look - 1, floor[0]), floor])
    csum = np.concatenate([[0.0], np.cumsum(lead)])
    return (csum[look:] - csum[:-look]) / look


def _limit(x: np.ndarray, sr: int, ceiling: float = CEILING) -> np.ndarray:
    # The clip only catches the cumulative sum's rounding, a few ulps.
    return np.clip(x * limiter_gain(x, sr, ceiling), -ceiling, ceiling)


def process(x: np.ndarray, preset: Preset | str, sr: int = SAMPLE_RATE) -> np.ndarray:
    """The chain on float samples in ``[-1, 1]``; the same length out, inside the ceiling."""
    p = PRESETS[preset] if isinstance(preset, str) else preset
    x = np.asarray(x, dtype=np.float64)
    if len(x) == 0:
        return x.copy()
    y = _shift(x, p, sr)
    if p.grit > 0:
        y = (1.0 - p.grit) * y + p.grit * np.tanh(p.drive * y) / np.tanh(p.drive)
    if p.double_mix > 0:
        d = int(p.double_ms * sr / 1000)
        y = y + p.double_mix * np.concatenate([np.zeros(d), y[: len(y) - d]])[: len(y)]
    if p.reverb_mix > 0:
        y = y + p.reverb_mix * _convolve(y, crypt_impulse(p.rt60_s, sr))
    y = _compress(y, p, sr)
    return _limit(y, sr)


def apply(pcm: bytes, preset: Preset | str, sr: int = SAMPLE_RATE) -> bytes:
    """PCM16LE in, PCM16LE out, the same number of samples."""
    x = np.frombuffer(pcm[: len(pcm) - len(pcm) % 2], dtype="<i2").astype(np.float64) / 32767.0
    y = process(x, preset, sr)
    return np.round(y * 32767.0).astype("<i2").tobytes()


def _read_wav(path: Path) -> tuple[bytes, int]:
    with wave.open(str(path), "rb") as w:
        if w.getnchannels() != 1 or w.getsampwidth() != 2:
            raise ValueError(f"{path.name} is not mono 16-bit")
        return w.readframes(w.getnframes()), w.getframerate()


def write_wav(path: Path, pcm: bytes, sr: int = SAMPLE_RATE) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with wave.open(str(path), "wb") as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(sr)
        w.writeframes(pcm)


def main(argv: Sequence[str] | None = None) -> int:
    parser = argparse.ArgumentParser(prog="shadowrunr_voice.fx")
    parser.add_argument("--preset", choices=sorted(PRESETS), default="light")
    parser.add_argument("input", type=Path)
    parser.add_argument("output", type=Path)
    args = parser.parse_args(argv)
    try:
        pcm, sr = _read_wav(args.input)
    except (OSError, ValueError, wave.Error) as exc:
        print(f"fx: {exc}", file=sys.stderr)
        return 1
    write_wav(args.output, apply(pcm, args.preset, sr), sr)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
