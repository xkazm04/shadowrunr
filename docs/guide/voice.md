# Malakor's voice

Malakor can speak his lines aloud. The voice is Kokoro (Apache-2.0), run on the operator's
machine by sherpa-onnx, one sentence at a time: kept loaded in the sidecar's worker processes
when its Python has sherpa-onnx (the warm engine), otherwise by `sherpa-onnx-offline-tts` once
per sentence. Nothing is sent anywhere. The voice is optional and off by default. With it off, degraded or slow, the stream
runs exactly as it does without one, and the brain never waits on audio.

Malakor's own voice is an original blend of stock Kokoro voices, `malakor-a`, under a light
lich effects chain. It is made on each machine from a recipe in the repo, and it is described in
[Malakor's voice: the blend and the lich chain](#malakors-voice-the-blend-and-the-lich-chain).

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

## The warm engine (recommended)

Run as a binary, the engine loads Kokoro for every sentence: about half a second of each render
is the load. The warm engine keeps the model loaded instead. The sidecar starts one worker
process per `render_workers` slot, each holding its own `sherpa_onnx.OfflineTts`, loads them all
before its `hello`, and hands each sentence to a free worker. It needs the `sherpa-onnx` Python
package (which bundles its own onnxruntime and espeak-ng; it is still the operator's install,
never the repo's) and the same Kokoro files under `<engine dir>/kokoro/`. The `bin/` binary is
the fallback, and optional when the warm engine runs: `voice check` lists it as `missing` (lower
case), and the daemon spawns the sidecar without it and lets the `hello` decide. The voice is
`ready` only if the hello names `engine: warm`. Any other hello is `degraded`, saying that
neither engine can run and quoting the sidecar's reason.

Make one venv for the voice with uv, outside the repo:

```
uv venv <voice venv> --python 3.12
uv pip install --python <voice venv> sherpa-onnx onnxruntime numpy
```

Point `[voice] python` at its interpreter (`<voice venv>/Scripts/python.exe` on Windows,
`<voice venv>/bin/python` elsewhere). The same venv has numpy and onnxruntime, so it also gives
the mouth shapes.

**Which engine runs.** `--engine auto` (the default) takes the warm engine when the sidecar's
Python can import sherpa_onnx and numpy and Kokoro's files are there. Otherwise it runs the
binary per sentence, and if no warm worker loads it falls back to the binary too.
`--engine warm` demands the warm engine (the voice is dead without it), and `--engine process`
always runs the binary. The `hello` has no field for the engine, so its `reason` ends with it:
`engine: warm (3 workers loaded in 2.1 s)`, or `engine: process (the warm engine is off:
sherpa_onnx is not installed)`. `shadowrunr voice say` prints that line when the sidecar is
ready, and the stage shows it beside the voice state.

**What is the same.** The audio is the engine's own: the same sample rate and PCM16, and sherpa's
pause cut, replayed sample for sample when the mouth shapes are on, exactly as for the binary.
The token ids for the mouth shapes come from the same debug print. Kokoro's decoder adds a little
noise of its own, so no two renders of a sentence are bit-identical with either engine; the
lengths match within a few samples and the token ids match exactly.

**Why processes, not threads.** sherpa's `generate` releases the GIL, so threads would run in
parallel, but sherpa prints the token ids only to the native stderr, which threads would share.
A worker process gives each render a stream of its own and a priority class of its own.

**Cancelling.** A sentence cannot be stopped inside the model. A cancelled line's sentences that
have not started never start; a sentence already rendering finishes below normal priority and
its result is thrown away, and its worker is free again when it ends. A sentence past
`render_timeout_ms` kills its worker.

**Dead and broken workers.** The pool supervises its workers. A worker that dies, or is killed
for its timeout, is respawned by the pool in the background from the moment it is dead, so the
model load overlaps the sentences the other workers are speaking; no sentence ever waits for a
load, and a sentence's render timeout never includes one. A slot whose loads fail 3 times within
60 s is quarantined: the sidecar's log names it once, with its last output, and the pool carries
on with the workers it has. At startup the pool keeps every worker that loads and respawns the
rest, so one bad load does not cost the warm engine: the hello then says `engine: warm (2 of 3
workers loaded in 2.1 s)`. Only when no worker loads does `auto` fall back to the binary. With
every slot quarantined each sentence fails at once with `the warm engine is not running`.

**Memory.** Each worker holds its own copy of the model: about 520 MB working set and 1 GB
committed per worker (3 workers: about 1.6 GB resident).

## Mouth shapes (optional)

The face moves its mouth through 15 shapes (the Oculus visemes). The sidecar works them out from
Kokoro's own token durations. This needs:

- **Kokoro's durations subgraph**: a small `.onnx` cut from `model.onnx` that predicts only how
  long each token lasts. It runs in tens of milliseconds per sentence.
- **A Python with numpy and onnxruntime.** The sidecar itself needs only the standard library.
  The warm engine's venv above already has both; for the binary alone, make a venv:

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
# voices_file = "<a blended voices file>"
# voices_dir = "~/.shadowrunr/voices"
# fx = "none"
max_queue = 6
max_lag_ms = 15000
render_timeout_ms = 30000
lines_in_flight = 3
render_workers = 3
# render_threads = 2
```

| Key | Default | Meaning |
|---|---|---|
| `enabled` | `false` | Off: `/voice` says `off` and nothing is started. |
| `python` | `python` | The interpreter that runs `python -m shadowrunr_voice`. |
| `sidecar` | `tools/voice` | The directory that holds the `shadowrunr_voice` package. |
| `engine_dir` | none | The sherpa-onnx TTS home described above. |
| `durations_model` | none | Kokoro's durations subgraph. Without it there are no mouth shapes. |
| `voice`, `sid` | `af_heart`, `3` | The voice and its verified speaker id. A wrong sid speaks as somebody else. |
| `voices_file` | none | The voices file `sid` is read from (`--voices`): a name in `voices_dir`, or an absolute path. Absent: the model's own `kokoro/voices.bin`. |
| `voices_dir` | `~/.shadowrunr/voices` | Where a `voices_file` name is found. Relative paths resolve against this file's directory; `~/` is your home. |
| `fx` | none | The effects chain over each sentence (`--fx`): `none`, `light` or `heavy`. |

The character package's `[voice]` (`characters/<id>/character.toml`) overrides `voice`, `sid`,
`voices_file` and `fx` field by field (`characters.md`); the rest, `voices_dir` included, are
the machine's. Malakor's package names `malakor-a`, sid 52 of `malakor-voices.bin`, fx `light`.
| `max_queue` | `6` | Lines waiting to render. Past this, the oldest is skipped (`queue_full`). |
| `max_lag_ms` | `15000` | A line whose first audio cannot start this soon after its `say` is skipped (`late`). |
| `render_timeout_ms` | `30000` | One sentence's render, at most. Past it, the line is skipped (`error`). |
| `lines_in_flight` | `3` | Lines handed to the sidecar at once, so it can render ahead into the next line. Their audio still goes out in order. |
| `render_workers` | `3` | Engine runs the sidecar renders at once (`--workers`); on the warm engine, the loaded workers. |
| `render_threads` | none | Each engine run's threads (`--threads`). Absent: 2 for the binary, 4 for the warm engine (measured below). |

## Providers: Kokoro and ElevenLabs

A character is spoken by one of two providers:

- **`kokoro`** (the default): the local sidecar described in this guide.
- **`elevenlabs`**: the ElevenLabs cloud API, one request per sentence
  (`POST /v1/text-to-speech/{voice_id}/with-timestamps?output_format=pcm_24000`), split into
  sentences by the sidecar's own rules. It sends no mouth shapes: the stage moves the mouth from
  loudness.

### The key

The ElevenLabs key lives only in the daemon's environment, in the variable that
`[voice.elevenlabs] api_key_env` names (default `ELEVENLABS_API_KEY`). It is never in a config
file, a log, an RPC result or the console. The daemon reads it at each request and sends it only
in the `xi-api-key` header.

```toml
[voice.elevenlabs]
api_key_env = "ELEVENLABS_API_KEY"   # the env var's name, never the key
model = "eleven_v4_turbo"            # used when the character's choice names no model
base_url = "https://api.elevenlabs.io"
timeout_ms = 20000                   # one request, at most
```

### Who speaks a character

The effective choice is built field by field:

1. the daemon's `[voice]` (the file's own kokoro fields);
2. the package's `[voice]` over it (`provider`, the kokoro fields, and `[voice.elevenlabs]` with
   `model` and `speed`);
3. the operator's local override, `<data>/characters/<id>/voice.json` (gitignored), which wins.

```json
{
  "provider": "elevenlabs",
  "kokoro": { "voice": "malakor-a", "sid": 52 },
  "elevenlabs": { "voice_id": "<a voice in your account>", "model": "eleven_v4_turbo", "speed": 1.08 }
}
```

The override is written by the console (Characters > Voice, `voice.settings.set`) or by hand. Only
the chosen provider's table is read; the other is kept, so switching back keeps its settings. An
ElevenLabs `voice_id` normally lives only in the override, so sharing a package never shares it.
Speeds must be 0.5 to 2.0 for Kokoro and 0.7 to 1.2 for ElevenLabs; an ElevenLabs voice without
a speed speaks at 1.08. The kokoro `speed` is not passed to the sidecar: Kokoro Plus takes its
speed per line, from the delivery.

**A change takes effect at the next stream start.** Each run's `stream_start` resolves the
streaming character's choice. When it differs from the voice that is speaking, the daemon replaces
the renderer before the run's first line: it starts a new sidecar with the new arguments, or an
ElevenLabs renderer. There is no daemon restart. At daemon start the voice is the one for
`[sim] character`. The `/voice` status names the provider (`provider`) and the voice (`voice`:
the Kokoro voice id, or the ElevenLabs voice's name).

### Never a fallback

A provider never falls back to another. When ElevenLabs refuses the key (401/403) the voice is
`degraded` with "elevenlabs: key rejected"; a missing key is `degraded` with the env var's name.
A network error or a timeout skips that line with `error`. Captions carry the show, and the
status says why the voice is silent.

### Delivery

The brain's delivery cue (`[sarcasm 0.8 slow] ...`) reaches the provider:

- **Kokoro Plus** turns it into speed, pauses, emotion blends and mild pitch (the sidecar).
- **ElevenLabs** opens each sentence with an inline audio tag: menace `[menacing]`, glee
  `[excited]`, deadpan `[deadpan]`, hushed `[whispers]`, outrage `[outraged]`, sarcasm
  `[sarcastic]`, weary `[sighs]`, triumph `[proclaiming]`, tender `[softly]`. A neutral line, or
  an intensity under 0.4, gets no tag. The pace has no tag: pauses are punctuation. The voice's
  `speed` goes in `voice_settings`; a model that refuses it is asked without it, and the daemon
  says so once on stderr.

### Preview, bench and verify (the console)

- `voice.settings.get {character}`: the package, override and effective choices, a label
  ("ElevenLabs Malakor (crypt king alt1), eleven_v4_turbo", "Kokoro malakor-a (sid 52), fx
  light") and each provider's readiness. The key state is `not_needed`, `missing`, `set`,
  `verified` or `rejected`, never the key. Kokoro's readiness here checks the engine's files and
  the voices file; `voice.verify` runs the whole `voice check`.
- `voice.settings.set {character, choice}`: writes the override (atomic, at most 4 KB, only
  inside `<data>`); `choice: null` clears it. An unknown character is `invalid_params`.
- `voice.verify {provider}`: for ElevenLabs, one `GET /v1/user`. 401/403 is `rejected`; a
  network error, a 429 or a key without account read permission leaves it `set`, with the
  reason. The last verdict and its `checked_ms` stay in memory and come back in
  `voice.settings.get`.
- `voice.preview {character, text, delivery?}`: speaks one line, at most 400 characters, with
  the character's effective choice. The text passes the brain's output gate first; a refused line
  is `invalid_params` with the gate's reason. It renders through a renderer of its own (a second
  sidecar for Kokoro), never the show's queue, one preview at a time (`busy` otherwise).
- `voice.bench {character}`: renders the package's `voice/bench.toml` (`[[line]] id, text,
  emotion, intensity, pace`) the same way, one line at a time. A line that fails carries its
  `error`, and the rest go on. A package without the file is `not_found`.

## The sidecar

```
python -m shadowrunr_voice --engine-dir <dir> [--durations <onnx>] [--voices <file>] --sid 3 --voice af_heart [--fx none|light|heavy] [--render-timeout-ms N] [--workers N] [--threads N] [--engine auto|warm|process]
```

The package is found from the `sidecar` directory (as the working directory, or on
`PYTHONPATH`). It speaks JSON lines on stdio, one object per line:

- First, `hello`: the protocol version, the voice, the voices file's name (`voices`, never its
  path), the `sid`, the `fx` preset, the sample rate (24000), whether mouth shapes are on, and a
  `reason`: what is missing, then the engine in use. On the warm engine it comes once every
  worker has loaded.
- **The voices file.** `--voices` replaces `kokoro/voices.bin` for every reader of it: the warm
  workers, the binary and the durations model behind the mouth shapes. A file that is missing,
  is not the stock file's size, or lacks `--sid` among its speakers makes the voice dead: the
  hello's `reason` says why (`the voices file malakor-voices.bin is missing`) and names no
  engine, and every `say` fails with it. Nothing falls back to another voice.
- **The effects chain.** `--fx light` or `heavy` runs `shadowrunr_voice.fx` over each sentence
  after the pause cut and after its mouth shapes are worked out on the dry audio. The chain
  keeps the sample count exactly, so the mouth timeline holds. It needs numpy; without it the
  voice is dead too (`fx light needs numpy, which is not installed`), never silently dry.
- For each `say`, the line is cut into sentences. A first sentence over about 120 characters is
  cut at a comma, so the first audio starts early. Each sentence is one render, and each
  sentence goes out as one `audio` message: mono PCM16 in base64, plus its mouth shapes in
  milliseconds.
- **The render pool.** Up to `--workers` renders (default 3) run at once, each with
  `--threads` threads (default 2 for the binary, 4 for the warm engine). Sentences start in the order they arrived, across lines: every
  sentence of a line before the next line's first, so the next line starts rendering while the
  current one's last sentence still is. The output stays in order (lines as they arrived, each
  line's sentences in order): a sentence that finishes early waits for its turn.
- **Head of line first.** The sentence about to be sent next always takes the first free
  worker, and it runs at normal priority. A look-ahead run (any later sentence) starts at below
  normal priority on Windows and is raised to normal once it is the one being waited for (a warm
  worker sets its own priority class when the sidecar tells it), so
  rendering ahead never takes CPU from the sentence the audience is waiting to hear. (Elsewhere
  every run has the same priority.)
- A failed or timed-out render is an `error` for that sentence, and the rest of the line is
  dropped (its later sentences still rendering are killed, or on the warm engine discarded).
  `cancel` kills the named lines' renders in flight (the warm engine discards them) and drops
  their sentences that have not started.
- It exits when its stdin closes. stdout carries the protocol only; its log goes to stderr.

The text reaches the binary as one argument in an argument list, never through a shell. On
Windows, characters the console code page cannot carry are folded first, so the engine always
reads the same single argument. A warm worker gets the text as a JSON line on its stdin.

Measured on the development machine with the binary (16 logical cores, CPU only, October 2026,
n = 4 lines of three sentences each): the first audio arrives 2.0 to 2.2 s after the `say`. Each
sentence takes 2.0 to 2.3 s to render for 2.0 to 2.5 s of audio, about half a second of which is
the model load that every engine run pays, so the engine only just keeps ahead of playback.
Mouth shapes add 20 to 50 ms per sentence.

With the warm engine (same machine, October 2026, 8 physical cores and 16 logical, CPU only,
sentences of 60 to 110 characters): a worker loads in 1.7 to 2.0 s alone, and 3 workers load
together in 2.0 to 3.7 s. A sentence alone renders at RTF 0.32 with 4 threads (0.23 with 8, 0.41
with 3), against about 0.8 for the binary with 2. In a burst of 12 sentences, 3 workers x 4
threads made 6.7 to 6.9 s of audio a second, 2 x 4 made 5.4, 3 x 3 made 5.8 to 6.2, one 8-thread
engine made 2.0 to 4.4, and the binary (3 x 2) made 2.7 to 3.4. Hence 3 workers (the
`render_workers` default) of 4 threads: 12 threads on 8 cores, with every look-ahead below normal
priority so the oversubscription falls on sentences nobody is waiting for yet.

Replays of two 5-minute sandbox shows (`shadowrunr voice replay`, `max_lag_ms` 15000, 3 workers;
n = 1 to 3 per row, run back to back with the binary on a shared machine, with its CPU load):

| Show | Engine | Lines voiced | First audio p50 / p90 | Machine CPU load (mean) |
|---|---|---|---|---|
| paced (24 lines) | warm | 24/24, 24/24, 24/24 | 1.6 / 2.0 s, 1.9 / 3.3 s, 5.3 / 7.3 s | 30%, 74%, 98% |
| paced (24 lines) | binary | 24/24, 20/24, 21/24 | 3.3 / 5.3 s, 3.7 / 6.2 s, 3.8 / 9.2 s | 39%, 70%, 65% |
| unpaced (91 lines) | warm | 91/91 | 2.3 / 3.4 s | 49% |
| unpaced (91 lines) | binary | 87/91 | 5.9 / 11.9 s | 67% |

The first audio is under 2 s at the median while the machine has room; under a saturated CPU the
voice still keeps every line, but each sentence waits for cores like everything else.

The pool's defaults (3 workers x 2 threads) come from replays of a 5-minute sandbox show
(`shadowrunr voice replay`, below; 8 physical cores, October 2026, n = 1 to 2 replays per point,
under a shared machine's background load, so treat them as provisional). Malakor's 91 lines in
that show are about 630 s of speech for 248 s of show. One worker makes about 170 to 190 s of
audio in the 248 s and drops about 65 lines; three workers make 290 to 365 s and drop about 40.
No render speed fixes that: the stage can play at most the show's own length, so most of what is
said can never be heard. Pacing the brain to the voice is the cure, not more workers.

### The brain paces itself to the voice

The brain keeps a speech clock: every line it puts on air advances it by the line's characters
at 18 a second (Kokoro `af_heart` measured about 18.3) plus a 250 ms pause. While more than
1.5 s of speech is still queued, a beat holds (`beat_plan` `hold`, reason `speaking: N.N s
queued`) and makes no model call; a safety beat and the closing beat never wait. Each beat asks
for at most about 2 sentences and 260 characters (about 14 s of speech), and lines past that are
dropped whole, the last first (`moderation {output, pace_budget, trimmed}`). These are
`BrainTuning.pace` values; the rate is measured, the pause, the lead and the budget are
assumptions to tune against voiced sandbox runs. The sandbox's `say_gap` counts nobody speaking on
the same clock, so a long line followed by its drain is not a silence.

Tests: `python -m unittest` from `tools/voice` (standard library only; no engine, no models).
The real-engine smoke test runs only when `SHADOWRUNR_VOICE_ENGINE_DIR` names the engine dir, and
it checks the mouth shapes too when `SHADOWRUNR_VOICE_DURATIONS` names the durations subgraph and
the interpreter has numpy and onnxruntime.

## Malakor's voice: the blend and the lich chain

Malakor speaks as `malakor-a`, an original voice: 0.6 of `am_onyx` and 0.4 of `bm_lewis`, two
stock Kokoro v1.0 voices (Apache-2.0) mixed by weight, under the `light` effects chain. The
owner picked it blind on 2026-10-22 from four lettered candidates (two blends, each with the
`light` and `heavy` chains; `python -m shadowrunr_voice.bench`). No real person's voice is
used, and nothing is cloned.

**The blend.** Kokoro reads a voice as a table of style rows per speaker in `voices.bin` (raw
float32, 53 speakers x 510 rows x 256). `python -m shadowrunr_voice.blend` writes a copy of the
stock file with each blend of a recipe summed row by row into a slot of its own. sherpa-onnx
refuses a voices file of any other size, so a blend replaces a speaker the show never uses:
`malakor-a` takes sid 52 (`zm_yunyang`), and `malakor-b`, the other candidate, sid 51. The
engine and the mouth shapes' durations model read the same file and the same sid, so the
render speed and the mouth sync are those of any stock voice. The recipe is
`tools/voice/recipes/malakor.toml`; the blended file is generated, local, and never committed.

**The lich chain.** `light` drops the pitch 2.5 semitones and the formants to 0.92 in one
spectral pass, adds a little `tanh` grit and a presence lift, a 0.9 s crypt reverb from an
impulse generated from a fixed seed, then a compressor and a look-ahead limiter
(`tools/voice/shadowrunr_voice/fx.py`). The same input always gives the same bytes, and the
output has exactly as many samples as the input.

**Making the voices file on a new machine.** From `tools/voice`, with any Python 3.11+ (the
blend needs only the standard library):

```
python -m shadowrunr_voice.blend --voices <engine dir>/kokoro/voices.bin --recipe recipes/malakor.toml
```

It writes `~/.shadowrunr/voices/malakor-voices.bin` (the recipe's name plus `-voices.bin`)
and `malakor-voices.bin.json` beside it, and prints each blend's sid and the output's sha256.
`--out <file>` writes elsewhere; then point `[voice] voices_dir` (or `voices_file`) at it. The
same stock file and recipe give the same bytes. From the stock kokoro-multi-lang-v1_0
`voices.bin` (sha256 `8a77c0d3...`), `malakor.toml` makes a file of sha256 `71b97216...`
(written twice, on 2026-10-21 and 2026-10-22, byte for byte the same). A file of another size
or a missing one leaves the voice `degraded` with the reason; the daemon never falls back to
another voice.

**What it costs** (one 8-core host, CPU only, 4 threads per render, n = 5 timed renders of each
of 3 lines, 2026-10-21): synthesis on `malakor-a` runs at RTF 0.27 to 0.28 against 0.25 to 0.29
for `af_heart`; the `light` chain takes 37 to 134 ms for 3.7 to 10.2 s of audio (RTF 0.010 to
0.013), in the sender, after the mouth shapes. Each line's mouth timeline ended exactly where its
audio did. `malakor-a` speaks about 6 to 8% slower than `af_heart` (3.73 against 3.46 s on the
short line).

Replays of a 10-minute sandbox show (56 lines; `shadowrunr voice replay`, 3 warm workers,
mouth shapes on; 2026-10-22, stock and `malakor-a` + `light` interleaved, n = 2 each, on a
machine shared with other jobs):

| Voice | Lines voiced | First audio p50 / p90 | Audio made |
|---|---|---|---|
| `af_heart` | 56/56, 56/56 | 2.0 / 2.8 s, 2.3 / 3.4 s | 391 s |
| `malakor-a`, fx `light` | 56/56, 56/56 | 2.3 / 3.1 s, 3.2 / 4.0 s | 410 s |

Driving the sidecar directly on the three bench lines (n = 5 each, interleaved, same day): the
engine rendered at RTF 0.39 for `af_heart` and 0.37 for `malakor-a`, and from `say` to the
line's last sentence (render, chain and send) took 0.31 against 0.29 of the line's audio at the
median. The blend and the chain cost no measurable speed; the spread between runs is the
machine's load.

**The next step: fine-tuning.** The blend is the baseline. For production quality the next
phase is fine-tuning an original designed voice (no cloning) if the blend proves not distinct
enough; nothing for it is built yet.

## Disclosure

While the voice is on, the stage shows "synthetic voice". The voice is `malakor-a`, an original
blend of stock Kokoro voices (kokoro-multi-lang-v1_0, Apache-2.0) under the lich effects chain
(above). It is no real person's voice, and voice cloning is out of scope. Malakor is an original
character.

## The daemon and the CLI

With `[voice] enabled = true`, `shadowrunr daemon` starts one voice service. It spawns the
sidecar (`python -m shadowrunr_voice` in `[voice] sidecar`) with arguments from `[voice]`:
`--engine-dir`, `--durations` (when set), `--voices` (the `voices_file`, when set), `--sid`,
`--voice`, `--fx` (when set), `--render-timeout-ms`, `--workers` and `--threads` (when set);
the engine is the sidecar's `auto` choice. Once the sidecar is ready the status `reason` starts
with the voice its hello names, then the engine: `malakor-a (sid 52), fx light, voices
malakor-voices.bin; engine: warm (3 workers loaded in 3.3 s)` (the fx is left out when it is
`none`, and the file when it is the model's own `voices.bin`). The
startup line ends with `(voice on)`. With the voice off, nothing is spawned and `/voice` says
`off`.

### Check the setup first

```
shadowrunr voice check
```

```
[voice] is enabled; malakor speaks with Kokoro malakor-a (sid 52), fx light
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

The rows are for the provider `[sim] character` resolves to. For ElevenLabs they are the key's
env var (set or missing, never its value), the `voice_id` and the model:

```
item              state    detail
elevenlabs key    ok       ELEVENLABS_API_KEY is set
elevenlabs voice  ok       voice_id <id>
elevenlabs model  ok       eleven_v4_turbo, speed 1.08
```

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
malakor speaks with Kokoro malakor-a (sid 52), fx light
voice ready in 865 ms, mouth shapes on
part 0  at  1845 ms   1067 ms audio   10 visemes   13 chars  I am Malakor.
part 1  at  4047 ms   2382 ms audio   24 visemes   41 chars  Kneel before the ledger of doom, mortals.
part 2  at  6217 ms   1336 ms audio   16 visemes   23 chars  Your socks are forfeit.
3 part(s), 4785 ms of audio, 50 visemes, 77 chars, first part after 1845 ms, all in 6219 ms
wrote <data>\voice\say-<ms>.wav
```

This speaks the line once, without a daemon, whether or not `[voice] enabled` is set, in the
character's effective voice (`--character`, default `[sim] character`; `--provider kokoro|elevenlabs`
picks the provider, with that provider's settings from the character's choice). The line passes
the brain's output gate first, and may open with a delivery cue (`[menace 0.8 slow] Kneel.`).
Each sentence prints as it arrives: when it arrived after the `say`, its audio length, its mouth
shapes, the characters sent to the provider and its text. `--out` with no path writes
`<data>/voice/say-<ms>.wav`, and `--out PATH` writes to PATH.

| Exit | `voice say` |
| --- | --- |
| 0 | the whole line was spoken |
| 1 | the voice did not start, the gate refused the line, or a sentence failed or timed out |
| 2 | usage error |

### Measure with a replay

```
shadowrunr voice replay runs/sandbox/<run>/transcript.jsonl [--workers N] [--lines-in-flight N] [--threads N] [--speed X] [--json] [--progress]
```

This pushes a recorded run's events again, at their recorded `ts_ms` (scaled by `--speed`),
into a live run that the voice service follows, through the real sidecar with this `[voice]`
(the flags override its keys). No model is called: the lines are the transcript's. It prints
one row: the lines spoken, `late`, `queue_full` and failed, the audio made, the first-audio
latency (from a `say` to its first sentence at the follower, p50 and p90), and a model of the
stage's player (sentences played back to back, at most 6 s queued ahead): the sentences it
drops, the audio heard, and the time from a `say` to its first sound. Run it on a quiet machine;
the engine shares the CPU with whatever else runs.

With no compare flag that one row is all it prints. To choose between configs, compare them in
one invocation instead of assembling rows by hand:

```
shadowrunr voice replay <transcript.jsonl> --arms "workers=2,3 flight=1,3" --repeat 3 [--baseline workers=3,flight=3] [--json]
```

- `--arms` is a grid: `key=v1,v2` over `workers`, `flight` (lines in flight) and `threads`, every
  value at least 1. The example is four configs.
- `--baseline` is the config the arms are measured against, one value per key. By default it is
  `[voice]` with the flags above.
- `--repeat N` replays every config N times. The repeats are interleaved: each round runs every
  config once before the next round starts, so a change in machine load falls on all of them, not
  on whichever config ran last.

Each figure prints as its median with its range and n, for example `1.9 s (1.6-5.3, n=3)`.
Latencies count only the repeats where a line spoke. Under the table, each arm's lines are diffed
against the baseline's. A line's outcome is its majority across the repeats; ties go to the
outcome seen first. Each changed line is listed with its seq, both outcomes and its text (cut to
60 characters), in one of three groups:

- **saved**: the baseline did not speak it, this arm did.
- **lost**: the baseline spoke it, this arm did not.
- **changed**: neither spoke it, for different reasons (`late`, `queue_full`, `error`, `partial`,
  `unresolved`).

These are the lines an audience would gain or miss. A difference in the totals alone can come
from load. `--json` prints `{"baseline": {...}, "arms": [...]}`: each config's spreads, its
`outcomes` by seq, and for an arm its `saved`, `lost` and `changed`. The plain `--json` report
also carries each line's outcome, as `outcomes`. A malformed `--arms`, `--baseline` or
`--repeat 0` is a usage error (exit 2), reported before anything runs.

| Exit | `voice replay` |
| --- | --- |
| 0 | the replay ran (whatever it measured) |
| 1 | the transcript could not be read, or the sidecar did not start |
| 2 | usage error |

### What the daemon does with a line

- **Only followed live runs are voiced.** That includes sandbox sessions. Replays and foil
  lines are never voiced. A run is voiced while at least one `/voice` client follows it. Each
  Malakor `say` becomes the line `<run_id>:<seq>`, and its sentences go out as `utterance`
  frames with the ids `<run_id>:<seq>:<part>`.
- **`max_queue`** (default 6): lines waiting to render. When a new line arrives past it, the
  oldest waiting line is `skipped` with `queue_full`.
- **`lines_in_flight`** (default 3): lines with the sidecar at once, beyond the queue. The
  sidecar renders ahead into them while the first is still going out. Each line's frames wait
  for the line before it, so a run's utterances stay in seq order.
- **`max_lag_ms`** (default 15000): the clock starts when the `say` reaches the daemon. A line
  whose first sentence has not reached the daemon by then is cancelled and `skipped` with
  `late`, even while it is still waiting its turn behind another line in flight (its skip goes
  out in its turn). The sidecar sends lines in order, so a line's first sentence arrives only
  after every line ahead of it is done: the time those lines take counts against it. A line
  already that old when it leaves the queue is skipped without rendering.
- **`render_timeout_ms`** (default 30000): one sentence, at most, counted from the sentence
  before it or from the line's turn, whichever is later. A sentence that fails or
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
| `degraded` | Kokoro's files are missing (nothing is spawned, and nothing retries until the config changes), the sidecar's `hello` names no engine because the voice cannot speak (its voices file is missing or the wrong size, or its fx needs numpy; the reason is the hello's, after the voice, and nothing retries), the binary is missing and the `hello` does not name the warm engine, the sidecar exited, or renders keep failing |

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
- The stage also reports on the socket: `{"played": {...}}` when a part's audio starts,
  `{"stage_dropped": {...}}` when it drops a whole line, and `{"face": {...}}` for the face it
  draws. They go to the run's `voice.jsonl` (below), are never answered, and never change the
  follow.
- **Malformed client frames are ignored.** The daemon logs each one on one stderr line. The
  socket only ever carries `status`, `utterance` and `skipped`, and the current follow is kept.
- **A slow client loses frames; it never slows the voice.** Each socket has a queue of 16
  frames. When an utterance does not fit, that utterance and the rest of its line are dropped
  for that client only. The client then gets one `skipped {reason: "queue_full"}` from the
  first dropped part, as soon as there is room. Other clients are unaffected.
- On daemon shutdown the socket closes with code 1001 and the reason `daemon shutting down`.

### What the voice did: `voice.jsonl`

A sandbox session's directory holds `voice.jsonl`: one line per voice event, in the order they
happen, on the session's stream clock (the transcript's `ts_ms` clock).

```
{"ts_ms":11950,"seq":4,"event":"queued","text":"Before the minions arrive, ..."}
{"ts_ms":13233,"seq":4,"event":"rendered","part":0,"last":true,"duration_ms":4633,"text":"Before the minions arrive, ..."}
{"ts_ms":13325,"seq":4,"event":"played","part":0}
{"ts_ms":41020,"seq":22,"event":"skipped","part":0,"reason":"late"}
```

| `event` | Written when | Keys |
| --- | --- | --- |
| `queued` | the line entered the voice's queue | `text`, `provider`, `delivery` (when the line carried a cue) |
| `rendered` | the daemon sent a sentence's `utterance` | `part`, `last`, `duration_ms`, `text`, `provider`, `chars`, `timings` |
| `skipped` | the daemon skipped the line from `part` | `part`, `reason` (`late`, `queue_full`, `error`, `unfollowed`) |
| `played` | the stage started playing a part | `part` |
| `stage_dropped` | the stage dropped the whole line | `reason` (`stage_cap`) |
| `face` | the stage changed the face it draws | `reason` (`rig`, `silhouette`); `seq` is 0 |

**Usage.** `chars` is what the part sent to its provider, the unit ElevenLabs bills; sum it over
a run's `rendered` lines for the stream's spend. Nothing caps it. `timings` says where the part's
timing came from: `durations` (Kokoro's mouth timeline), `provider` (ElevenLabs' character
timestamps came back; only that fact is logged, the mouth still follows loudness) or `none`.

The log is a ledger: every `queued` line ends in its `rendered` parts (the last one with
`last: true`) or in one `skipped`. `unfollowed` is the log's own reason, for a line whose run
nobody followed any more before it was spoken; no frame carries it. A line with no end yet is
still owed, and the recorder waits for it (recording.md). A feeder that falls behind the run's
events reads what it missed from the run's history, so no line goes unlogged. Logs written
before `queued` and `last` existed read as before: a `rendered` part with no `last` ends its
line.

`seq` is the `say` event's seq. The stage's times are its own wall clock; the daemon converts
them with the session's `started_ms`. While the session runs that clock is estimated from the
events as they arrive, and when the session ends the file is rewritten on the report's exact
`started_ms`. The log never holds up the stream: a full queue drops lines, and a write that
fails is logged once on stderr. To line the log up with a recording, see `recording.md`.

### The face rig

`GET /avatar/{file}` serves the rig of the character on air: `<data>/characters/<id>/avatar/rig/<file>`, else the shared `<data>/avatar/rig/<file>` (see `characters.md`). The name must be one segment of
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

### The depth map

The depth face (`?avatar=depth`) displaces the rig's layers by a depth map that sits beside
`layers.json`: `depth.png` (16-bit grayscale, near is bright) and `depth.json` (how to read it
back, the method, a plane depth per crown shard, and the sha256 of every input). With no depth
map the stage draws the glow face. The map is cut from the portrait too, so it stays under
`data/` and is never committed. `tools/avatar/depth_map.py` makes it; the same inputs give the
same bytes. It needs numpy and Pillow. There are two methods:

- **`analytic-v1`** (the default) builds the depth from the rig's geometry: an ellipsoid head
  carved where the skull is dark, the jaw a little forward, the shards on a ring, and a torso slab.

  ```
  python tools/avatar/depth_map.py <data>/avatar/rig
  ```

- **`learned-v1`** runs a monocular depth model, Depth-Anything-V2-Small, over the recomposed
  portrait. The head takes the model's depth, calibrated so the head ellipsoid's centre and
  rim keep their `analytic-v1` depths (so a turn keeps its size), with the model's forward lean
  taken off. The torso is the same slab, carved with the model's relief (the folds, the collar
  and the clasp). The shards keep their planes. `depth.json` adds the model's id, revision and
  sha256, and the calibration.

The model runs in a Python that has onnxruntime (the voice venv has it). Nothing downloads
the model. Use the Small model only: it is Apache-2.0, while Base, Large and Giant are
CC-BY-NC. Fetch `onnx/model.onnx` from `onnx-community/depth-anything-v2-small` at revision
`4472b7362082ad9968fee890ca0f1e5aca36b93d` (99,060,839 bytes, sha256
`afb6a5c28f3b6bf1618c6e43f02073ef9dfdc70e937502d51603e57b0a1df10c`). Put it at
`~/.shadowrunr/models/depth-anything-v2-small/model.onnx`, or point `--model` or
`SHADOWRUNR_DEPTH_MODEL` at it. A file with another sha256 is refused. Never put the model in
the repo.

```
python tools/avatar/depth_map.py <data>/avatar/rig --method learned-v1 --model-python <voice venv python>
```

That one command runs three steps. You can also run them by hand, without `--model-python`:

1. `depth_map.py --method learned-v1` writes the model's input beside the rig. This is
   `learned.in.npy`: the portrait at 518x518, normalised. The command stops with exit 2.
2. `<voice venv python> tools/avatar/learned_depth.py <data>/avatar/rig` runs the model on CPU
   with its threads pinned. It writes `learned.out.npy` and `learned.json` (the model and the
   input it read).
3. Run `depth_map.py --method learned-v1` again. It checks that the output was made from the
   current input, then writes the map.

`SHADOWRUNR_DEPTH_PYTHON` stands in for `--model-python`. If the rig changes, step 1 writes a
new input and waits for the model again. The `learned.*` files are cut from the portrait too,
so they stay beside the rig.

On the M8 rig, `analytic-v1` takes about 2 s. `learned-v1` takes about 6 s in all, of which
the model step is about 1 s.

To compare two maps on the same frames, use the lab in `apps/stage/lab/` (`depth.html`,
`record.mjs`). It is a development harness and is not shipped.

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
- When the stage falls more than 6 s behind, it drops the oldest line that has not started, all
  of its sentences. It never cuts a line it has started, and it keeps the newest line. It never
  talks later and later.
- A headless browser (the clip recorder, any automation) is never voiced. Clips stay silent.

### Looking at the face without a voice (dev builds)

Run `pnpm -C apps/stage dev` and add `?face=` to the URL:
- `?face=demo` loops a scripted line;
- `?face=rest` holds the face at rest;
- `?face=aa` (or any other mouth shape: `oh`, `E`, `PP`, ...) holds that shape mid-speech.

This parameter does nothing in a production build.
