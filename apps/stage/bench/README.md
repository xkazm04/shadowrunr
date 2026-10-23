# The face bench

Measures what each face costs to stream, on the same inputs, so a face can be chosen on look
against cost. Every face (`cut`, `depth`, `relit`, `lich3d`) runs through its real `startFace`
(`src/avatar/variants.ts`) fed the bench script (`?face=bench`, `BENCH_SCRIPT` and
`benchSources` in `src/avatar/demo.ts`): 24 s of speech, every emotion and gesture, and the look
moving between the host, the content and the chat. It is a development harness and is not shipped.

## Setup

- A rig with a depth map and the lich GLB in `data/avatar/rig/` (`layers.json`, `depth.png`,
  `depth.json`, `lich.glb`, `lich.json`). The bench serves it with CORS on `--rig-port`.
- Playwright, which the stage does not depend on: `npm i playwright` in a scratch directory and
  set `BENCH_PLAYWRIGHT` to that directory. It uses Playwright's own Chromium
  (`npx playwright install chromium` once) on the GPU (ANGLE D3D11).
- `nvidia-smi` for the GPU figures, `ffmpeg` for the clips. Process CPU sampling is Windows only.

## Commands

Run from `apps/stage`. Run one script at a time, on a quiet machine: they share the ports and
each measures the whole machine.

```
node bench/run-all.mjs --repeat 3 [--shadowrunr <exe> --voice-config <toml> --rounds 3] [--skip clips]
node bench/frames.mjs  [--faces cut,depth,relit,lich3d] [--sizes 440,1024] [--repeat 3] [--build]
node bench/process.mjs [--sizes 440,1024] [--seconds 60]
node bench/ready.mjs   [--size 440] [--repeat 5]
node bench/voice.mjs   --shadowrunr <exe> --voice-config <toml> [--rounds 2]
node bench/sheet.mjs   [--size 440]
node bench/clips.mjs   [--from 4000] [--seconds 12] [--fps 30]
node bench/report.mjs
node --test bench/lib/stats.test.mjs
```

Common flags: `--out` (default `<repo>/runs/avatar-3d/bench`, also `BENCH_OUT`), `--rig-dir`
(default `<repo>/data/avatar/rig`), `--port` (5184, the page) and `--rig-port` (5284).

## What each number means

- **GPU ms** (`frames.mjs`): the GPU time of everything a face submitted in one frame, from an
  `EXT_disjoint_timer_query_webgl2` TIME_ELAPSED query around the frame's step. It does not
  include the browser compositing the canvas, which every face pays alike. Percentiles are over
  the frames the face drew (a face asleep draws nothing). `gl.finish()` returns at once in
  Chrome, so a CPU timer around render + finish measures nothing; the timer query is the method.
- **CPU ms** (`frames.mjs`): the main-thread time of the same step: the drivers, the look, the
  performance channels and the WebGL calls. The page is served cross-origin isolated so the
  timer has microsecond resolution.
- The frame-cost run is frame-stepped on an injected clock at 60 Hz and paced by
  `requestAnimationFrame`, so every face draws exactly the same 1440 frames. The GPU clocks down
  at low load, so each config runs `--repeat` times, interleaved; the report gives the median run
  and the range.
- **Process CPU %** (`process.mjs`): a `vite build` of the bench page with the face on the page's
  real frame clock, looping the script. CPU time of the browser's processes by role, sampled at
  1 Hz for 60 s, in percent of ONE core. GPU utilisation and VRAM come from `nvidia-smi` and are
  device-wide: the VRAM delta is against the 5 s before the browser started.
- **Ready ms** (`ready.mjs`): navigation to `data-face="ready"`, cold (a fresh browser and
  profile per run, the rig served `no-store`), and `startFace` to ready (the face's own load: its
  chunk, the rig, the depth map or the GLB, the shaders).
- **First audio** (`voice.mjs`): `shadowrunr voice say --provider kokoro` (local, no quota) on 5
  fixed lines with each face running and with no browser before and after; the summary line's
  `first part after N ms`.
- **Machine busy %**: the share of all cores busy while a row ran, from `os.cpus()`. The bench
  also records the top foreign processes before each run (`foreignTop` in the JSON). Read CPU
  figures taken under foreign load as relative between faces, not as a dedicated machine's.
- A figure that was not measured is left out (a dash in the report), never written as 0.

## Outputs

`frames.json`, `process.json`, `ready.json`, `voice.json`, `raw/` (every frame of every run),
`bench.md` (the table), `sheet.png` (rows = channels, columns = faces, each face at the same
pose, look and performance values), `clips/` (`<letter>-corner.mp4` at 440 px,
`<letter>-host.mp4` at 1024 px, `lineup.mp4`, `lineup-host.mp4`) and `key/key.json` (letter to
face; keep it closed until the clips are ranked).
