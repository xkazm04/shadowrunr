#!/usr/bin/env node
// Record one clip candidate to MP4 by stepping the stage's clip mode frame by frame.
//
//   node scripts/record-clip.mjs --dist <apps/stage/dist> --clip <clip.json> --out <draft.mp4>
//        --poster <poster.png> --chrome <chrome|msedge exe> --ffmpeg <ffmpeg exe> [--fps 30]
//
// Zero dependencies (Node 24: global fetch and WebSocket). It serves the built stage and the
// clip on 127.0.0.1, starts a headless Chromium with the DevTools protocol, opens
// `index.html?clip=/__clip/clip.json&record=1`, and for every frame calls
// `window.__clip.seek(ms)` (which resolves once the frame is committed, painted and its CSS
// animations settled) and captures a JPEG that is piped into ffmpeg (libx264, yuv420p). The
// poster PNG is the frame at the middle of the longest Malakor caption. Chromium and its temp
// profile are removed on exit, error or signal. Exit 0 only when the MP4 was written; any
// failure is one clear line on stderr and exit 1. The clip factory (crates/shadowrunr-clips)
// verifies the result with ffprobe: 1080x1920 at the clip's duration.

import { spawn } from "node:child_process";
import { createReadStream, existsSync, mkdtempSync, readFileSync, rmSync, statSync, writeFileSync } from "node:fs";
import { createServer } from "node:http";
import { tmpdir } from "node:os";
import { extname, join, normalize, resolve, sep } from "node:path";

const W = 1080;
const H = 1920;
const READY_TIMEOUT_MS = 30_000;
const STEP_TIMEOUT_MS = 20_000;

function args(argv) {
  const out = { fps: "30" };
  for (let i = 0; i < argv.length; i++) {
    const k = argv[i];
    if (!k.startsWith("--")) throw new Error(`unexpected argument ${k}`);
    const v = argv[i + 1];
    if (v === undefined) throw new Error(`${k} needs a value`);
    out[k.slice(2)] = v;
    i++;
  }
  for (const k of ["dist", "clip", "out", "poster", "chrome", "ffmpeg"]) {
    if (!out[k]) throw new Error(`--${k} is required`);
  }
  const fps = Number(out.fps);
  if (!Number.isInteger(fps) || fps < 1 || fps > 60) throw new Error("--fps must be 1..60");
  return { ...out, fps };
}

const TYPES = {
  ".html": "text/html; charset=utf-8",
  ".js": "text/javascript; charset=utf-8",
  ".css": "text/css; charset=utf-8",
  ".json": "application/json; charset=utf-8",
  ".svg": "image/svg+xml",
  ".png": "image/png",
  ".woff2": "font/woff2",
  ".woff": "font/woff",
  ".ttf": "font/ttf",
};

/** Serve `dist` and the clip file (at /__clip/clip.json) on 127.0.0.1:<random>. */
function serve(dist, clipFile) {
  const root = resolve(dist);
  const server = createServer((req, res) => {
    const path = decodeURIComponent((req.url ?? "/").split("?")[0]);
    let file;
    if (path === "/__clip/clip.json") file = clipFile;
    else {
      file = normalize(join(root, path === "/" ? "index.html" : path));
      if (file !== root && !file.startsWith(root + sep)) {
        res.writeHead(403).end();
        return;
      }
    }
    if (!existsSync(file) || !statSync(file).isFile()) {
      res.writeHead(404).end();
      return;
    }
    res.writeHead(200, { "content-type": TYPES[extname(file).toLowerCase()] ?? "application/octet-stream", "cache-control": "no-store" });
    createReadStream(file).pipe(res);
  });
  return new Promise((ok, fail) => {
    server.once("error", fail);
    server.listen(0, "127.0.0.1", () => ok({ server, port: server.address().port }));
  });
}

/** Start Chromium; resolves with the DevTools HTTP port once it prints its ws URL. */
function launchChrome(exe, profile) {
  const child = spawn(
    exe,
    [
      "--headless=new",
      "--remote-debugging-port=0",
      `--user-data-dir=${profile}`,
      "--hide-scrollbars",
      "--mute-audio",
      `--window-size=${W},${H}`,
      "--force-device-scale-factor=1",
      "--no-first-run",
      "--no-default-browser-check",
      "--disable-extensions",
      "--disable-background-timer-throttling",
      "--disable-renderer-backgrounding",
      "about:blank",
    ],
    { stdio: ["ignore", "ignore", "pipe"], windowsHide: true },
  );
  return new Promise((ok, fail) => {
    let err = "";
    const timer = setTimeout(() => fail(new Error(`Chromium printed no DevTools URL in 20 s: ${err.trim().slice(-300)}`)), 20_000);
    child.once("error", (e) => {
      clearTimeout(timer);
      fail(new Error(`could not start Chromium (${exe}): ${e.message}`));
    });
    child.once("exit", (code) => {
      clearTimeout(timer);
      fail(new Error(`Chromium exited early (code ${code}): ${err.trim().slice(-300)}`));
    });
    child.stderr.on("data", (d) => {
      err += d.toString();
      const m = /DevTools listening on ws:\/\/127\.0\.0\.1:(\d+)\//.exec(err);
      if (m) {
        clearTimeout(timer);
        child.removeAllListeners("exit");
        ok({ child, port: Number(m[1]) });
      }
    });
  });
}

/** A minimal DevTools-protocol session over one target's WebSocket. */
async function cdp(wsUrl) {
  const ws = new WebSocket(wsUrl);
  await new Promise((ok, fail) => {
    ws.addEventListener("open", ok, { once: true });
    ws.addEventListener("error", () => fail(new Error(`DevTools socket ${wsUrl} failed`)), { once: true });
  });
  let id = 0;
  const waiting = new Map();
  ws.addEventListener("message", (m) => {
    const msg = JSON.parse(typeof m.data === "string" ? m.data : Buffer.from(m.data).toString());
    if (msg.id !== undefined && waiting.has(msg.id)) {
      const { ok, fail } = waiting.get(msg.id);
      waiting.delete(msg.id);
      if (msg.error) fail(new Error(`${msg.error.message} (${msg.error.code})`));
      else ok(msg.result);
    }
  });
  ws.addEventListener("close", () => {
    for (const { fail } of waiting.values()) fail(new Error("DevTools socket closed"));
    waiting.clear();
  });
  const send = (method, params = {}, timeoutMs = STEP_TIMEOUT_MS) =>
    new Promise((ok, fail) => {
      const n = ++id;
      const timer = setTimeout(() => {
        waiting.delete(n);
        fail(new Error(`${method} timed out after ${timeoutMs} ms`));
      }, timeoutMs);
      waiting.set(n, {
        ok: (r) => (clearTimeout(timer), ok(r)),
        fail: (e) => (clearTimeout(timer), fail(e)),
      });
      ws.send(JSON.stringify({ id: n, method, params }));
    });
  return { send, close: () => ws.close() };
}

async function evaluate(page, expression) {
  const r = await page.send("Runtime.evaluate", { expression, awaitPromise: true, returnByValue: true });
  if (r.exceptionDetails) {
    throw new Error(`page error: ${r.exceptionDetails.exception?.description ?? r.exceptionDetails.text}`);
  }
  return r.result.value;
}

function posterMs(clip) {
  let best = null;
  for (const c of clip.captions ?? []) {
    if (c.speaker === "malakor" && (!best || c.text.length > best.text.length)) best = c;
  }
  const t = best ? (best.start_ms + best.end_ms) / 2 : clip.duration_ms / 2;
  return Math.max(0, Math.min(clip.duration_ms - 1, t));
}

async function main() {
  const a = args(process.argv.slice(2));
  const clipFile = resolve(a.clip);
  const clip = JSON.parse(readFileSync(clipFile, "utf8"));
  if (!(clip.duration_ms > 0)) throw new Error("clip.json has no positive duration_ms");
  if (!existsSync(join(a.dist, "index.html"))) throw new Error(`no stage build at ${a.dist} (run pnpm -C apps/stage build)`);
  const durationS = clip.duration_ms / 1000;
  const frames = Math.ceil(durationS * a.fps);
  const posterFrame = Math.min(frames - 1, Math.round((posterMs(clip) / 1000) * a.fps));

  const profile = mkdtempSync(join(tmpdir(), "shadowrunr-clip-"));
  let chrome = null;
  let server = null;
  let ffmpeg = null;
  let page = null;
  const cleanup = () => {
    try {
      page?.close();
    } catch {
      /* already closed */
    }
    if (chrome && chrome.exitCode === null) {
      if (process.platform === "win32") {
        // Chromium's helper processes hold the profile open; end the whole tree we started.
        spawn("taskkill", ["/pid", String(chrome.pid), "/T", "/F"], { stdio: "ignore", windowsHide: true });
      } else chrome.kill("SIGKILL");
    }
    if (ffmpeg && ffmpeg.exitCode === null) ffmpeg.kill("SIGKILL");
    server?.close();
  };
  const removeProfile = async () => {
    for (let i = 0; i < 20; i++) {
      try {
        rmSync(profile, { recursive: true, force: true });
        return;
      } catch {
        await new Promise((r) => setTimeout(r, 250));
      }
    }
  };
  for (const sig of ["SIGINT", "SIGTERM"]) {
    process.once(sig, () => {
      cleanup();
      process.exit(1);
    });
  }

  try {
    const s = await serve(a.dist, clipFile);
    server = s.server;
    const c = await launchChrome(a.chrome, profile);
    chrome = c.child;
    const url = `http://127.0.0.1:${s.port}/index.html?clip=${encodeURIComponent("/__clip/clip.json")}&record=1`;
    const res = await fetch(`http://127.0.0.1:${c.port}/json/new?${encodeURIComponent("about:blank")}`, { method: "PUT" });
    if (!res.ok) throw new Error(`DevTools /json/new: HTTP ${res.status}`);
    const target = await res.json();
    page = await cdp(target.webSocketDebuggerUrl);
    await page.send("Page.enable");
    await page.send("Runtime.enable");
    await page.send("Emulation.setDeviceMetricsOverride", { width: W, height: H, deviceScaleFactor: 1, mobile: false });
    await page.send("Page.navigate", { url });

    const t0 = Date.now();
    for (;;) {
      const st = await evaluate(page, "(() => { const c = window.__clip; return c ? { ready: c.ready, error: c.error, d: c.durationMs } : null; })()");
      if (st?.error) throw new Error(`the stage could not load the clip: ${st.error}`);
      if (st?.ready) break;
      if (Date.now() - t0 > READY_TIMEOUT_MS) throw new Error("the stage did not become ready in 30 s");
      await new Promise((r) => setTimeout(r, 100));
    }

    const ffErr = [];
    ffmpeg = spawn(
      a.ffmpeg,
      [
        "-hide_banner", "-loglevel", "error", "-y",
        "-f", "image2pipe", "-framerate", String(a.fps), "-c:v", "mjpeg", "-i", "-",
        "-c:v", "libx264", "-pix_fmt", "yuv420p", "-preset", "veryfast", "-crf", "20",
        "-movflags", "+faststart", "-r", String(a.fps), "-t", durationS.toFixed(3),
        resolve(a.out),
      ],
      { stdio: ["pipe", "ignore", "pipe"], windowsHide: true },
    );
    ffmpeg.stderr.on("data", (d) => ffErr.push(d.toString()));
    const ffDone = new Promise((ok, fail) => {
      ffmpeg.once("error", (e) => fail(new Error(`could not start ffmpeg (${a.ffmpeg}): ${e.message}`)));
      ffmpeg.once("exit", (code) => (code === 0 ? ok() : fail(new Error(`ffmpeg exited ${code}: ${ffErr.join("").trim().slice(-400)}`))));
    });
    // A dead ffmpeg must fail the run, not hang a write.
    let ffFailed = null;
    ffDone.catch((e) => (ffFailed = e));

    for (let i = 0; i < frames; i++) {
      if (ffFailed) throw ffFailed;
      const ms = Math.min(clip.duration_ms, (i * 1000) / a.fps);
      await evaluate(page, `window.__clip.seek(${ms})`);
      const shot = await page.send("Page.captureScreenshot", { format: "jpeg", quality: 90, fromSurface: true });
      const buf = Buffer.from(shot.data, "base64");
      if (!ffmpeg.stdin.write(buf)) await new Promise((r) => ffmpeg.stdin.once("drain", r));
      if (i === posterFrame) {
        const png = await page.send("Page.captureScreenshot", { format: "png", fromSurface: true });
        writeFileSync(resolve(a.poster), Buffer.from(png.data, "base64"));
      }
    }
    ffmpeg.stdin.end();
    await ffDone;
    process.stdout.write(`recorded ${frames} frames (${durationS.toFixed(2)} s at ${a.fps} fps) to ${a.out}\n`);
  } finally {
    cleanup();
    await removeProfile();
  }
}

main().then(
  () => process.exit(0),
  (e) => {
    process.stderr.write(`record-clip: ${e instanceof Error ? e.message : String(e)}\n`);
    process.exit(1);
  },
);
