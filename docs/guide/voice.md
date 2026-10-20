# Malakor's voice

Malakor can speak his lines aloud. The voice is Kokoro (Apache-2.0), run on the operator's
machine by sherpa-onnx's `sherpa-onnx-offline-tts`, one sentence at a time. Nothing is sent
anywhere. The voice is optional and off by default. With it off, degraded or slow, the stream
runs exactly as it does without one, and the brain never waits on audio.

The pieces:

| Piece | What it does |
|---|---|
| The sidecar (`tools/voice`, `python -m shadowrunr_voice`) | Renders a line a sentence at a time and works out each sentence's mouth shapes. The daemon starts it and talks to it over stdio. |
| The daemon (`GET /voice`) | Hands Malakor's live `say` lines to the sidecar, bounds the queue, and pushes each sentence to the stage. |
| The stage | Plays the audio and moves the face. The audio is the one clock: the mouth follows playback time. |

The wire is `docs/spec/contract.md`, appendix (voice).

## Installing the engine and the model

Nothing is bundled. The repo ships no engine, no model and no espeak-ng data. The engine links
espeak-ng, which is GPL, so Shadowrunr only calls the operator's own install. The model files are
large, and they are a separate download with their own licence.

1. Download a sherpa-onnx release for your platform from the sherpa-onnx GitHub releases page
   (`k2-fsa/sherpa-onnx`). Copy `sherpa-onnx-offline-tts` (`.exe` on Windows) and the libraries
   next to it into `<engine dir>/bin/`.
2. Download the Kokoro multi-lang v1.0 model (`kokoro-multi-lang-v1_0`) from the same releases
   page (the `tts-models` release). Unpack it into `<engine dir>/kokoro/`.

The sidecar expects this layout:

```
<engine dir>/
  bin/sherpa-onnx-offline-tts[.exe]
  kokoro/model.onnx
  kokoro/voices.bin
  kokoro/tokens.txt
  kokoro/espeak-ng-data/
  kokoro/lexicon-us-en.txt      (used when present)
```

Check the install by hand:

```
<engine dir>/bin/sherpa-onnx-offline-tts --kokoro-model=<engine dir>/kokoro/model.onnx \
  --kokoro-voices=<engine dir>/kokoro/voices.bin --kokoro-tokens=<engine dir>/kokoro/tokens.txt \
  --kokoro-data-dir=<engine dir>/kokoro/espeak-ng-data --sid=3 --output-filename=test.wav " I am Malakor."
```

If a file is missing, the sidecar still starts. It says which file is missing in its `hello`,
replies `error` to every line, and the daemon reports the voice as degraded.

## Mouth shapes (optional)

The face moves its mouth through 15 shapes (the Oculus visemes). The sidecar works them out from
Kokoro's own token durations. This needs:

- **Kokoro's durations subgraph**: a small `.onnx` cut from `model.onnx` that predicts only how
  long each token lasts. It runs in tens of milliseconds per sentence.
- **A Python with numpy and onnxruntime.** The sidecar itself needs only the standard library,
  so make a venv for it:

  ```
  python -m venv <venv>
  <venv>/bin/pip install numpy onnxruntime      (on Windows: <venv>\Scripts\pip)
  ```

  or `pip install ./tools/voice[visemes]`. Point `[voice] python` at that venv's interpreter.

The durations subgraph is extracted once, with the `onnx` package, from the `tokens`, `style`
and `speed` inputs to the durations output of `model.onnx`. Keep it outside the repo.

Without either piece, the sidecar says `visemes: false` and why in its `hello`, and it still
speaks. The stage then moves the mouth with the loudness of the audio.

## `[voice]` in `shadowrunr.toml`

Every key is optional. Paths are machine-local: keep them in your own `shadowrunr.toml` (which is
not committed), and resolve relative paths against that file's directory.

```toml
[voice]
enabled = true
python = "<venv python, with numpy and onnxruntime for mouth shapes>"
sidecar = "tools/voice"
engine_dir = "<engine dir: bin/ and kokoro/>"
durations_model = "<kokoro durations subgraph .onnx; absent = no mouth shapes>"
voice = "af_heart"
sid = 3
max_queue = 6
max_lag_ms = 8000
render_timeout_ms = 30000
```

| Key | Default | Meaning |
|---|---|---|
| `enabled` | `false` | Off: `/voice` says `off` and nothing is started. |
| `python` | `python` | The interpreter that runs `python -m shadowrunr_voice`. |
| `sidecar` | `tools/voice` | The directory that holds the `shadowrunr_voice` package. |
| `engine_dir` | none | The sherpa-onnx TTS home described above. |
| `durations_model` | none | Kokoro's durations subgraph. Without it there are no mouth shapes. |
| `voice`, `sid` | `af_heart`, `3` | The voice and its verified speaker id. A wrong sid speaks as somebody else. |
| `max_queue` | `6` | Lines waiting to render. Past this, the oldest is skipped (`queue_full`). |
| `max_lag_ms` | `8000` | A line whose first audio cannot start this soon after its `say` is skipped (`late`). |
| `render_timeout_ms` | `30000` | One sentence's render, at most. Past it, the line is skipped (`error`). |

## The sidecar

```
python -m shadowrunr_voice --engine-dir <dir> [--durations <onnx>] --sid 3 --voice af_heart [--render-timeout-ms N]
```

The package is found from the `sidecar` directory (as the working directory, or on
`PYTHONPATH`). It speaks JSON lines on stdio, one object per line:

- First, `hello`: the protocol version, the voice, the sample rate (24000), whether mouth shapes
  are on, and a `reason` when something is missing.
- For each `say`, the line is cut into sentences. A first sentence over about 120 characters is
  cut at a comma, so the first audio starts early. Each sentence is one engine run. The next
  sentence starts rendering as soon as the current one's audio is in, and each sentence goes out
  as one `audio` message: mono PCM16 in base64, plus its mouth shapes in milliseconds.
- A failed or timed-out render is an `error` for that sentence, and the rest of the line is
  dropped. `cancel` kills a line's render in flight and drops the queued lines it names.
- It exits when its stdin closes. stdout carries the protocol only; its log goes to stderr.

The text reaches the engine as one argument in an argument list, never through a shell. On
Windows, characters the console code page cannot carry are folded first, so the engine always
reads the same single argument.

Measured on the development machine (16 logical cores, CPU only, October 2026, n = 4 lines of
three sentences each): the first audio arrives 2.0 to 2.2 s after the `say`. Each sentence takes
2.0 to 2.3 s to render for 2.0 to 2.5 s of audio, about half a second of which is the model
load that every engine run pays, so the engine only just keeps ahead of playback. Mouth shapes add 20 to 50 ms per sentence.

Tests: `python -m unittest` from `tools/voice` (standard library only; no engine, no models).
The real-engine smoke test runs only when `SHADOWRUNR_VOICE_ENGINE_DIR` names the engine dir, and
it checks the mouth shapes too when `SHADOWRUNR_VOICE_DURATIONS` names the durations subgraph and
the interpreter has numpy and onnxruntime.

## Disclosure

While the voice is on, the stage shows "synthetic voice". The voice is Kokoro's stock `af_heart`
(sid 3 of kokoro-multi-lang-v1_0). It is no real person's voice, and voice cloning is out of
scope. Malakor is an original character.

## The daemon and the CLI

<!-- AV-2: `/voice`, the sidecar's lifecycle, the queue and the skips, and the CLI. -->

## The stage

<!-- AV-3: the face, the placeholder rig, the loudness fallback, and the disclosure badge. -->
