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

With `[voice] enabled = true`, `shadowrunr daemon` starts one voice service. It spawns the
sidecar (`python -m shadowrunr_voice` in `[voice] sidecar`) with arguments from `[voice]`:
`--engine-dir`, `--durations` (when set), `--sid`, `--voice` and `--render-timeout-ms`. The
startup line ends with `(voice on)`. With the voice off, nothing is spawned and `/voice` says
`off`.

### Check the setup first

```
shadowrunr voice check
```

```
[voice] is enabled (voice af_heart, sid 3)
item                   state    detail
engine                 ok       bin\sherpa-onnx-offline-tts.exe in engine_dir
kokoro model.onnx      ok       kokoro\model.onnx in engine_dir
kokoro voices.bin      ok       kokoro\voices.bin in engine_dir
kokoro tokens.txt      ok       kokoro\tokens.txt in engine_dir
kokoro espeak-ng-data  ok       kokoro\espeak-ng-data in engine_dir
python                 ok       Python 3.12.9
sidecar                ok       shadowrunr_voice imports
mouth-shape packages   ok       numpy, onnxruntime
durations model        ok       durations.onnx
voice: ready to speak, with mouth shapes
```

`MISSING` (upper case) is an item the voice needs. `missing` (lower case) only costs the mouth
shapes; the stage then moves the mouth from loudness. `--json` prints the items.

| Exit | `voice check` |
| --- | --- |
| 0 | everything needed to speak is present (mouth shapes are optional) |
| 1 | something needed is missing |
| 2 | usage error |

### Hear one line

```
shadowrunr voice say "I am Malakor. Kneel before the ledger of doom, mortals. Your socks are forfeit." --out
```

```
sidecar ready in 865 ms, mouth shapes on
part 0  at  1845 ms   1067 ms audio   10 visemes  I am Malakor.
part 1  at  4047 ms   2382 ms audio   24 visemes  Kneel before the ledger of doom, mortals.
part 2  at  6217 ms   1336 ms audio   16 visemes  Your socks are forfeit.
3 part(s), 4785 ms of audio, 50 visemes, first part after 1845 ms, all in 6219 ms
wrote <data>\voice\say-<ms>.wav
```

This runs the sidecar once, without a daemon, whether or not `[voice] enabled` is set. Each
sentence prints as it arrives: when it arrived after the `say`, its audio length, its mouth
shapes and its text. `--out` with no path writes `<data>/voice/say-<ms>.wav`, and `--out PATH`
writes to PATH.

| Exit | `voice say` |
| --- | --- |
| 0 | the whole line was spoken |
| 1 | the sidecar did not start, or a sentence failed or timed out |
| 2 | usage error |

### What the daemon does with a line

- **Only followed live runs are voiced.** That includes sandbox sessions. Replays and foil
  lines are never voiced. A run is voiced while at least one `/voice` client follows it. Each
  Malakor `say` becomes the line `<run_id>:<seq>`, and its sentences go out as `utterance`
  frames with the ids `<run_id>:<seq>:<part>`.
- **`max_queue`** (default 6): lines waiting to render. When a new line arrives past it, the
  oldest waiting line is `skipped` with `queue_full`.
- **`max_lag_ms`** (default 8000): the clock starts when the `say` reaches the daemon. A line
  whose first sentence has not rendered by then is cancelled and `skipped` with `late`. A line
  already that old when it leaves the queue is skipped without rendering.
- **`render_timeout_ms`** (default 30000): one sentence, at most. A sentence that fails or
  times out skips the rest of its line with `error`, from that part on. Three failed lines in a
  row turn the status `degraded` ("renders failing: ..."), and the next spoken line clears it.
- **The stream never waits.** The service reads each run's event broadcast. It never holds up
  the brain, the transcript or `/events`.

### Status and restarts

| State | When |
| --- | --- |
| `off` | `[voice] enabled = false` |
| `starting` | the sidecar is loading (its `hello` has not arrived) |
| `ready` | speaking; `visemes` says whether mouth shapes come with it, and `reason` says why not |
| `degraded` | the engine files are missing (nothing is spawned, and nothing retries until the config changes), the sidecar exited, or renders keep failing |

When the sidecar exits, any line in flight is skipped from the part it reached. The sidecar
restarts after 1 s, and the wait doubles on each exit up to 60 s. It restarts at most 3 times
in any 5 minutes; after that it waits for the window to clear, and `reason` gives the time
left.

The sidecar never outlives the daemon. On shutdown the daemon closes the sidecar's stdin (the
sidecar exits on EOF), waits 2 s, then kills it. If the daemon dies outright, the pipe closes
and the sidecar exits on its own. In a hard-kill test it was gone within 0.8 s.

### `/voice` for clients

- The current `status` is sent on connect, and again on every change.
- Nothing is followed until the client sends `{"follow": {}}` (the newest live run, moving to
  each new run as it starts) or `{"follow": {"run_id": "..."}}`. A named run that is not live
  yet is picked up when it starts.
- **Malformed client frames are ignored.** The daemon logs each one on one stderr line. The
  socket only ever carries `status`, `utterance` and `skipped`, and the current follow is kept.
- **A slow client loses frames; it never slows the voice.** Each socket has a queue of 16
  frames. When an utterance does not fit, that utterance and the rest of its line are dropped
  for that client only. The client then gets one `skipped {reason: "queue_full"}` from the
  first dropped part, as soon as there is room. Other clients are unaffected.
- On daemon shutdown the socket closes with code 1001 and the reason `daemon shutting down`.

### The face rig

`GET /avatar/{file}` serves `<data>/avatar/rig/<file>`. The name must be one segment of
`[A-Za-z0-9._-]` with no `..`, and end in `.json`, `.png` or `.webp`. Anything else, or a file
that is not there, is 404, and the stage draws its placeholder. Responses carry
`cache-control: no-cache`, so a re-split rig shows on the next load.

## Building the face rig

The face rig is a layer split of the owner's portrait. It is made offline by `tools/rig`
(`python -m shadowrunr_rig`) from a hand-measured region spec. For Malakor's M8 portrait the
spec is `characters/malakor/rig/m8.spec.json`.

**Never commit the portrait or any layer cut from it.** They are the owner's files, and both
live under `data/`, which is gitignored:
- the portrait is `<data>/avatar/portraits/m8/portrait.png`;
- the rig is written to `<data>/avatar/rig/`, where `/avatar/{file}` serves it.

The spec holds only polygons and points, and it is committed.

You need a Python with numpy and Pillow, plus torch for inpainting
(`pip install ./tools/rig[lama]`), and big-lama as TorchScript (`big-lama.pt`, Apache-2.0).
Nothing downloads the model. From `tools/rig`:

```
python -m shadowrunr_rig   --portrait <data>/avatar/portraits/m8/portrait.png   --spec ../../characters/malakor/rig/m8.spec.json   --out <data>/avatar/rig   --lama <path to big-lama.pt>
```

The run removes the old PNGs in `--out` and writes `layers.json` plus one PNG per layer. It
refuses to write a `layers.json` the stage would refuse. It then prints how closely the rest frame
recomposes the portrait. Reload the stage to see the new rig.

What the M8 rig holds, back to front:

| Layer | Kind | What it does |
|---|---|---|
| `background` | static | The near-black ground, with everything above it filled in. |
| `torso` | body | The robe, the collar and the quill-and-ledger clasp. It breathes. |
| `head` | head | The skull above the jaw, with the upper teeth and dark sockets. Behind the jaw it holds the dark mouth. |
| `mouth_glow` | aperture_glow | A faint teal light in the open mouth (drawn by the renderer). |
| `jaw` | aperture_lower | The lower jaw and teeth. It drops on open vowels. |
| `shard_*` (11) | float | The crown's obsidian shards. They drift. |
| `eye_l`, `eye_r` | eye | The teal soul-fire, as light. Blinks dim it and the voice warms it. |
| `monocle` | pulse | The ember ring and its sparks, as light. It pulses slowly. |

The rig's own lights are teal in the mouth and an ember ground line. The bust dissolves into
the ground line, so its robe never meets the edge of the slot.

`tools/rig/README.md` covers the spec format and the tests.

## The stage

The stage draws Malakor's face in its avatar slot: above his caption in the improv layouts, and
in the small circle beside the caption in SEG-02A. The face works without the voice. While a
`say` caption is on screen his head sways as he talks, and at rest he breathes and blinks.

### Which face is drawn

- **Live** (`?run=...`): the stage asks the daemon for `/avatar/layers.json`. If a rig is in
  `<data>/avatar/rig/`, that rig is drawn.
- **No rig, no daemon, or a replay:** the stage draws its procedural placeholder. This is a dark
  crowned skull with an ember mouth seam, verdigris eye lights and floating crown shards. It is
  painted in the browser, and no image file ships with it.
- **A rig that does not load or check:** the stage draws the placeholder and logs why, once, in
  the browser console. A plain 404 is not logged.
- **No WebGL2:** the face is not drawn, and the slot keeps the SVG silhouette.
- **Reduced motion** (the OS setting): the face is one still frame at rest.

### Turning the voice on

The voice plays only on live runs, and it is off by default.

1. Open the stage on a live run: `http://localhost:5178/?run=live`. Give `?ws=` if your daemon is
   not on `ws://127.0.0.1:7717/events`.
2. In the controls at the bottom of the page, click **Voice**. The click also unlocks audio,
   since browsers keep audio silent until a click. The setting is remembered in this browser.
3. Read the voice state beside the button:
   - `voice off` (with a reason): the toggle is off, or the stage is showing a replay;
   - `voice starting`: connecting to the daemon's `/voice`;
   - `voice ready`: the daemon's voice is up;
   - `voice degraded` (with a reason): the engine is missing, the socket is closed, or audio is
     blocked until a click.
4. While the voice is on, the disclosure badge reads "AI character · synthetic voice", and a
   "synthetic voice" label sits under the face.

**OBS / broadcast view.** `?broadcast=1` hides the controls, so add `?voice=1`, for example
`?run=live&broadcast=1&voice=1`. OBS's browser source normally plays audio without a click.
If audio is still blocked, the stage stays silent and shows `degraded`. Route the browser
source's audio in OBS as usual.

### What the stage does with the audio

- The stage follows the run it shows: it sends `{"follow": {"run_id"}}` and sends it again when
  the run changes.
- Each sentence is scheduled straight after the previous one, so the parts of a line play without
  gaps.
- The mouth follows the audio's own clock. The shape is read at the playback position (35 ms
  ahead) and the loudness from the output. When a sentence has no mouth shapes, the mouth follows
  loudness alone.
- When the stage falls more than 6 s behind, it drops the oldest sentence that has not started.
  It never talks later and later.
- A headless browser (the clip recorder, any automation) is never voiced. Clips stay silent.

### Looking at the face without a voice (dev builds)

Run `pnpm -C apps/stage dev` and add `?face=` to the URL:
- `?face=demo` loops a scripted line;
- `?face=rest` holds the face at rest;
- `?face=aa` (or any other mouth shape: `oh`, `E`, `PP`, ...) holds that shape mid-speech.

This parameter does nothing in a production build.
