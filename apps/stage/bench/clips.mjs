// Blind lettered clips: the same 12 s of the bench script (4 s .. 16 s: menace and its glare, glee
// at the chat, outrage's recoil and flare, the turn to the content, hushed leaning in) for every
// face, frame-stepped at 30 fps on the injected clock (every face takes `clock`, so a clip is the
// same frames whatever the machine does), at the 440 px corner and the 1024 px host room. Faces
// get the letters A..D in a random order; the key goes to its own folder.
//
//   node bench/clips.mjs [--faces cut,depth,relit,lich3d] [--from 4000] [--seconds 12] [--fps 30] [--ffmpeg ffmpeg]
//
// Writes <out>/clips/<L>-corner.mp4, <L>-host.mp4, lineup.mp4 (the corner clips, 2 x 2, letters
// burned in, no names), lineup-host.mp4, and <out>/key/key.json (letter -> face).

import { spawn } from "node:child_process";
import { randomInt } from "node:crypto";
import { writeFileSync } from "node:fs";
import { join } from "node:path";

import { benchPage, devServer, gpuBrowser, outDir, rigServer, settings } from "./lib/env.mjs";
import { shuffle } from "./lib/stats.mjs";

const s = settings();
const clipsDir = outDir(s, "clips");
const keyDir = outDir(s, "key");
const ffmpeg = s.args.ffmpeg ?? "ffmpeg";
const fps = Number(s.args.fps ?? 30);
const fromMs = Number(s.args.from ?? 4000);
const seconds = Number(s.args.seconds ?? 12);
const FRAMINGS = [
  { name: "corner", size: 440 },
  { name: "host", size: 1024 },
];

const LETTERS = "ABCDEFGH";
const order = shuffle(s.faces, () => randomInt(0, 1_000_000) / 1_000_000);
const key = Object.fromEntries(order.map((face, i) => [LETTERS[i], face]));

function run(cmd, argv, input) {
  return new Promise((ok, fail) => {
    const child = spawn(cmd, argv, { stdio: [input ? "pipe" : "ignore", "ignore", "pipe"] });
    let err = "";
    child.stderr.on("data", (d) => (err += d));
    child.on("error", fail);
    child.on("exit", (code) => (code === 0 ? ok() : fail(new Error(`${cmd} exited ${code}: ${err.slice(-800)}`))));
    if (input) input(child.stdin);
  });
}

/** Record one face at one framing to `file`: H.264, yuv420p, `fps`. */
async function record(page, face, letter, size, file) {
  const started = await page.evaluate((o) => window.__bench.start(o), { face, size, rig: rig.url, manual: true });
  if (started.state !== "ready" || started.fellBack) throw new Error(`${face} did not start as itself: ${JSON.stringify(started)}`);
  const dt = 1000 / fps;
  await page.evaluate((o) => window.__bench.advance(o), { steps: Math.round(fromMs / dt), dt });
  const frames = Math.round(seconds * fps);
  let slept = 0;
  await run(ffmpeg, ["-loglevel", "error", "-y", "-f", "image2pipe", "-framerate", String(fps), "-c:v", "mjpeg", "-i", "-", "-c:v", "libx264", "-preset", "slow", "-crf", "16", "-pix_fmt", "yuv420p", "-movflags", "+faststart", file], async (stdin) => {
    for (let f = 0; f < frames; f++) {
      const g = await page.evaluate((o) => window.__bench.grab(o), { steps: 1, dt, letter });
      if (!g.drew) slept++;
      if (!stdin.write(Buffer.from(g.url.split(",")[1], "base64"))) await new Promise((r) => stdin.once("drain", r));
    }
    stdin.end();
  });
  await page.evaluate(() => window.__bench.stop());
  return { frames, slept };
}

const rig = await rigServer(s.rigDir, s.rigPort);
const web = await devServer(s.port);
let gpu = null;
const made = [];
try {
  gpu = await gpuBrowser();
  for (const [letter, face] of Object.entries(key)) {
    for (const fr of FRAMINGS) {
      // a fresh page per clip: no face shares GL state or caches with another
      // the page's console names the face, so it is not echoed next to its letter
      const { context, page } = await benchPage(gpu.browser);
      await page.goto(web.url);
      await page.waitForFunction(() => !!window.__bench);
      const file = join(clipsDir, `${letter}-${fr.name}.mp4`);
      const r = await record(page, face, letter, fr.size, file);
      await context.close();
      made.push({ letter, framing: fr.name, size: fr.size, file, ...r });
      console.log(`clip ${letter}-${fr.name}: ${r.frames} frames (${r.slept} with the face asleep)`);
    }
  }
} finally {
  await gpu?.close();
  await web.close();
  await rig.close();
}

// the lineups: A B / C D, the letters already burned into each clip
const letters = Object.keys(key);
for (const [name, framing, scale] of [
  ["lineup.mp4", "corner", 440],
  ["lineup-host.mp4", "host", 720],
]) {
  const inputs = letters.flatMap((l) => ["-i", join(clipsDir, `${l}-${framing}.mp4`)]);
  const scaled = letters.map((_, i) => `[${i}:v]scale=${scale}:${scale}[v${i}]`).join(";");
  const layout = letters.map((_, i) => `${(i % 2) * scale}_${Math.floor(i / 2) * scale}`).join("|");
  const filter = `${scaled};${letters.map((_, i) => `[v${i}]`).join("")}xstack=inputs=${letters.length}:layout=${layout}:fill=black[out]`;
  await run(ffmpeg, ["-loglevel", "error", "-y", ...inputs, "-filter_complex", filter, "-map", "[out]", "-c:v", "libx264", "-preset", "slow", "-crf", "18", "-pix_fmt", "yuv420p", "-movflags", "+faststart", join(clipsDir, name)]);
}
writeFileSync(join(keyDir, "key.json"), JSON.stringify({ key, window: { fromMs, seconds, fps }, clock: "injected (frame-stepped)", at: new Date().toISOString() }, null, 1));
writeFileSync(join(clipsDir, "clips.json"), JSON.stringify({ window: { fromMs, seconds, fps }, clips: made.map(({ letter, framing, size, frames, slept }) => ({ letter, framing, size, frames, slept })) }, null, 1));
console.log(`clips in ${clipsDir}; key in ${join(keyDir, "key.json")}`);
