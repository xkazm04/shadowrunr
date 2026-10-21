# Recording the show

`shadowrunr stream sandbox --record` records a sandbox session in OBS Studio. OBS draws the
stage itself through a Browser Source, so its recording holds the picture and Malakor's voice
together, on one clock. There is no screen or desktop capture. The file stays in the session
directory with `recording.json`, which says where the session's stream clock sits in the file.

The pieces:

| Piece | What it does |
|---|---|
| OBS Studio, with obs-websocket v5 (built in since OBS 28) | Renders the stage in a browser source and records it. |
| The built stage, served on `localhost` | The broadcast view, voice on: `?run=live&broadcast=1&voice=1&ws=...`. |
| `shadowrunr stream sandbox --record` | Sets OBS up, starts recording before the session, stops it once the voice has played out and the end card is shown, and writes `recording.json`. |
| The daemon | Writes `voice.jsonl`: what the voice rendered and skipped, and what the stage played (voice.md). |

## Setting OBS up (once per machine)

1. Install OBS Studio (on Windows: `winget install OBSProject.OBSStudio`).
2. Turn its WebSocket server on, with authentication and a password. The server is off by
   default. Either use the dialog (Tools > WebSocket Server Settings: enable, keep
   authentication on, generate a password), or, **with OBS closed**, write OBS's own config file
   `<OBS config>/plugin_config/obs-websocket/config.json`:

   ```json
   {
       "alerts_enabled": false,
       "auth_required": true,
       "first_load": false,
       "server_enabled": true,
       "server_password": "<a long random password>",
       "server_port": 4455
   }
   ```

   `<OBS config>` is `%APPDATA%\obs-studio` on Windows and `~/.config/obs-studio` elsewhere.
   `first_load: false` stops OBS replacing the password with its own on the next start.
   Generate the password in a script that never prints it. It is a secret: it never goes in
   `shadowrunr.toml`, a command line or a log.
3. On a fresh install, skip OBS's auto-configuration wizard: with OBS closed, put
   `FirstRun=true` under `[General]` in `<OBS config>/user.ini` (and `global.ini`, for older
   versions). Otherwise the wizard opens on first start and waits for a click.
4. Start OBS. It can stay in the tray:

   ```
   obs64.exe --minimize-to-tray --disable-shutdown-check --disable-updater
   ```

   Start it from its own `bin\64bit` directory on Windows (OBS looks for its data relative to
   the working directory).

On first start, Windows may ask whether OBS may accept network connections. obs-websocket
listens on every interface; the recorder only talks to `127.0.0.1`, which works whatever you
answer. Denying the prompt keeps the port closed to the network.

### The password

The recorder reads it from the environment variable `[record] obs_password_env` names
(`SHADOWRUNR_OBS_PASSWORD` by default). When that variable is unset or empty, it reads
`server_password` from OBS's own `config.json` above, so on the machine OBS runs on nothing
else is needed. `shadowrunr record check` says which source it used, never the password.

## `[record]` in `shadowrunr.toml`

Every key is optional:

```toml
[record]
obs_host = "127.0.0.1"
obs_port = 4455
obs_password_env = "SHADOWRUNR_OBS_PASSWORD"
stage_url = "http://localhost:5178"   # where the built stage is served
```

`stage_url` must be `http://localhost:<port>`. The daemon only accepts browser origins on
`http://localhost:*`, so `127.0.0.1` would load the page but never connect.

## Which face is recorded: `[stage] avatar`

```toml
[stage]
avatar = "cut"   # "cut" (default), "glow" or "depth"
```

The stage draws the face with one of three renderers (`?avatar=`, `apps/stage/DESIGN.md`).
`[stage] avatar` picks the one the daemon's stage URLs ask for: both the OBS browser source's URL
and the sandbox's `stage_url` end in `&avatar=cut` (or `glow`, `depth`), and `recording.json`'s
`stage_url` shows it. The default is `cut`, the owner's pick from the 2026-10-21 recorded show;
the URL names it anyway, so a recording says which face it drew whatever the stage build's own
default. Any other value is a config error. The OBS URL comes
from the config `stream sandbox --record` loads, the sandbox's `stage_url` from the daemon's (read
when it starts), so give both the same file. `stream sandbox --record` prints the URL OBS records.

## Check OBS: `shadowrunr record check`

Probes OBS without recording anything:

```
obs-websocket  ws://127.0.0.1:4455
password from  OBS's obs-websocket config
reachable      yes (obs-websocket 5.7.4)
auth           ok
version        OBS 32.2.2 (rpc 1)
scene          "Shadowrunr Stage" present
recording      idle
stage          http://localhost:5178
```

| Exit | `record check` |
| --- | --- |
| 0 | OBS answered and accepted the password, and `stage_url` is a `localhost` origin |
| 1 | OBS is not reachable, refused the password, or `stage_url` is not usable |
| 2 | usage error |

## Recording a session

1. Run the daemon (`shadowrunr daemon`) with the voice on if you want it heard.
2. Build the stage and serve it on `localhost`, on the port in `stage_url`:

   ```
   pnpm -C apps/stage build
   pnpm -C apps/stage preview --port 5178 --strictPort --host localhost
   ```

3. Run the session with `--record`:

   ```
   shadowrunr stream sandbox --minutes 5 --record
   ```

What happens:

1. The command checks that the daemon answers, then that OBS does. **If OBS is not reachable,
   the session does not start** (exit 1), with a line saying what to do; it never runs
   unrecorded.
2. It sets OBS up, refusing if OBS is already recording:
   - video 1920x1080 base and output, 30 fps;
   - a scene `Shadowrunr Stage`, made the program scene;
   - in it, a browser source `Shadowrunr Stage Browser` (OBS names are unique across scenes and
     sources, so the two cannot share a name): the stage URL, 1920x1080 at 30 fps, audio
     controlled through OBS, restarted when it becomes active;
   - OBS's global Desktop Audio and Mic/Aux inputs muted, so only the stage is heard.
3. It gives the page 4 s to load and connect, then starts recording into
   `<runs>/sandbox/.recording/`, and only then starts the session.
4. When the session leaves the air (judging, or ended), the recorder waits for the voice to
   play out: the voice runs several seconds behind the events, so the sign-off is still being
   spoken when the session ends. It reads the session's `voice.jsonl` until every rendered part
   has played (or was dropped), the last one's audio is over, and nothing new has appeared for
   2 s, for at most 45 s. Then the recording runs 3 s more, with the end card on screen, and
   stops.
5. When the session has its report, the file moves into the session directory as
   `recording.<ext>` (OBS's own container: `mp4` with OBS's default hybrid MP4, else `mkv`),
   beside `recording.json`.
6. The watched checks are measured from the recording and `voice.jsonl` and written into the
   session's `session.json`; the checks table and the exit code are the merged report's (see
   [The watched checks](#the-watched-checks)).

If the session fails to start, the recording is stopped and deleted. Ctrl-C works as without
`--record`; a second Ctrl-C detaches the command and stops the recording.

## `recording.json`

```json
{
  "tool": "obs",
  "file": "<session dir>/recording.mp4",
  "started_wall_ms": 1792572627195,
  "stream_offset_ms": -542,
  "ended_wall_ms": 1792572885464,
  "width": 1920,
  "height": 1080,
  "fps": 30,
  "stage_url": "http://localhost:5178/?run=live&broadcast=1&voice=1&ws=ws://127.0.0.1:7717/events&avatar=cut"
}
```

`stream_offset_ms` is the stream-clock time of the file's first frame: `started_wall_ms` minus
the session's `started_ms`. It is negative because recording starts before the session. An
event at stream time `t` (a transcript `ts_ms`, or a `voice.jsonl` `ts_ms`) is at
`t - stream_offset_ms` in the file. For example, a `played` line at `ts_ms` 10720 with an offset
of -542 starts at 11.262 s into the recording.

## Checking a recording

```
ffprobe -v error -show_entries stream=codec_type,width,height,avg_frame_rate -show_entries format=duration recording.mp4
```

Expect a 1920x1080 video stream at 30 fps (H.264), an AAC audio stream, and a duration of the
session plus its edges: the time before the session starts (`-stream_offset_ms`), then after
it the voice playing out and 3 s more. A 4-minute session that ended at stream time 243.5 s
recorded 258.2 s: 0.5 s before, 10 s of sign-off still being spoken, the 3 s tail and the
daemon's status poll.

To check that the voice lines up with `voice.jsonl`, find speech onsets in the audio with
`ffmpeg -i recording.mp4 -af silencedetect=noise=-35dB:d=0.25 -f null -` (each `silence_end` is
an onset), map each `played` line to file time with `stream_offset_ms`, and compare. Only line
starts that follow a pause show as onsets. In the sessions measured so far, speech starts about
200 ms after its `played` time, at the head and the tail alike (no drift); about 70 ms of that is
the silence at the start of each Kokoro sentence.

## Clips from the recording

`shadowrunr clips scan --run <session dir>` reads the session's `voice.jsonl` and
`recording.json` (cli.md, `clips`), and `shadowrunr clips render <id>` cuts the recording:

- **Timing.** The window is on the voice's real timing: a line runs from its first `played`
  to its last part's `played` plus that part's duration. The cut opens up to 0.4 s before its
  first line, never into the line heard before it, and its 1.5 s reaction beat stops where the
  next heard line starts.
- **The A/V offset.** Speech starts about 200 ms after `played` (measured on
  `sbx-20261021-085027-5m`, within 25 ms, no drift). It is recorded in each candidate and
  manifest as `av_offset_ms`, with that provenance, and cuts are **not** shifted by it: the cut
  opens before `played`, so the first syllable is always inside it.
- **`cut-16x9.mp4`.** The recording from `window start - stream_offset_ms` for the window's
  length, re-encoded H.264/AAC (input seek plus re-encode, so the cut is frame-accurate),
  1920x1080 at the recording's frame rate. Nothing is burnt in: the stage's captions and its
  AI badge are in the picture. A SEG-02A cut is the exception: its ALLEGED header, grade badge
  and citation strip are burnt in, as the gate requires.
- **`cut-9x16.mp4`.** 1080x1920 at the same frame rate, on a dark ground:

  | Region | From the 1920x1080 stage (`apps/stage/src/layouts.ts`) | In the 9:16 frame |
  | --- | --- | --- |
  | The AI badge | the disclosure, x 1584-1904, y 30-82 | 480 px wide, centred, y 60 |
  | Improv (SEG-01 and the rest) | `avatar`, x 64-624, y 112-724 | full width, y 170-1350 |
  | Hexmap (SEG-02A) | the map's hexes, x 32-728, y 104-848; `avatar` x 40-164, y 864-988 | hexes full width from y 170; the face 300x300 bottom right of them |

  Below the picture the lines are burnt in from `captions-9x16.ass`: the speaker in the
  stage's verdigris, the line in capitals in its bone colour (the caption face's own Georgia
  fallback; Cinzel is a web font), one line at a time, on the cut's clock. A line the voice
  never said is set dim and italic.
- **Checks.** Both files are probed (`h264`, the right size, the window's duration within
  0.5 s); a failure leaves the poster frame and says why. The gate adds `handles_in_picture`
  (unmeasured) to a cut from the recording: the 16:9 picture is the broadcast, chat column
  included, so a person confirms the handles may be shown before approving.

The console plays both cuts on the clip's page (On Air, Clips) and the session's recording in
the Review player's watch pane, from the daemon's `GET /media/clips/<id>/<file>` and
`GET /media/runs/sandbox/<session>/recording.mp4` (byte ranges, so the player seeks).

## The watched checks

A recorded session's report carries four checks beside `say_gap`, `stage_ack` and the rest,
each `pass`, `FAIL` or `unmeasured` with a detail:

| Check | Reads | Passes when |
| --- | --- | --- |
| `voiced` | `voice.jsonl` | at least 90% of Malakor's lines played in full (no part skipped or dropped) |
| `face` | `voice.jsonl` `face` events | the rig was drawn for at least 95% of the session (unknown counts as silhouette) |
| `av_sync` | the recording | head and tail offsets within -45/+125 ms of the run's constant, drift at most 40 ms |
| `dead_air` | the recording | no silence longer than 15 s between the cold open and the sign-off |

`av_sync` uses the method above (`silencedetect` at -35 dB over 0.25 s, each onset matched to the
nearest `played` time within 600 ms). The constant (the median offset, about 200 ms through OBS)
is reported, not judged: what is judged is how far the head and the tail sit from it, and the
drift between them. `dead_air` runs `silencedetect` at -50 dB, at least 1 s, and reports the
longest stretch on the show clock. The thresholds and how each is measured are in the contract
(spec/contract.md, sandbox appendix).

**They gate only a recorded session.** `stream sandbox --record` starts the session with
`record: true`, and then an unmeasured watched check fails the gate like any other. A session
started without `--record` reports them (`unmeasured` when the voice was off) and gates on the
other checks as before; the checks table marks them `(not gating: unrecorded)`.

### Recomputing them: `shadowrunr stream check`

```
shadowrunr stream check <runs>/sandbox/<session id> [--write]
```

Recomputes the four watched checks from the run dir (ffmpeg from `[clips] ffmpeg`), prints the
checks table and the gate, and with `--write` writes them into `session.json`. It needs no
daemon.

| Exit | `stream check` |
| --- | --- |
| 0 | the session passes its gate |
| 2 | usage error, or the run dir has no readable `session.json` |
| 6 | the session fails its gate |

## Troubleshooting

| Symptom | Cause |
|---|---|
| `OBS is not reachable` | OBS is not running, or its WebSocket server is off. |
| `OBS refused the connection: the password was not accepted` | The variable holds a different password from OBS's; unset it to use OBS's own config. |
| `it asks for a password and none is configured` | Neither the variable nor OBS's config file has one. |
| `OBS is already recording` | Stop the recording in OBS first; the recorder never takes over one it did not start. |
| The recording shows a blank stage | The stage is not served at `stage_url`, or it is on `127.0.0.1` rather than `localhost`. |
| The recording is silent | The voice is off or `degraded` (see voice.md), or the browser source's audio is muted in OBS's mixer. |
