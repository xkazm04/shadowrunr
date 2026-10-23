// What every bench script shares: where things are, Playwright (from BENCH_PLAYWRIGHT: the stage
// does not depend on it), a GPU Chromium, the rig's CORS server, the Vite dev server and preview,
// and the machine-load and nvidia-smi probes.

import { spawn, spawnSync } from "node:child_process";
import { existsSync, mkdirSync, readFileSync } from "node:fs";
import { createServer } from "node:http";
import { createRequire } from "node:module";
import { cpus } from "node:os";
import { dirname, extname, join, normalize, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

import { machineBusy } from "./stats.mjs";

export const BENCH = resolve(dirname(fileURLToPath(import.meta.url)), "..");
export const STAGE = resolve(BENCH, "..");
export const REPO = resolve(STAGE, "..", "..");

/** `--key value` pairs and `--flag` booleans into an object (values stay strings). */
export function args(argv = process.argv.slice(2)) {
  const out = {};
  for (let i = 0; i < argv.length; i++) {
    const k = argv[i];
    if (!k.startsWith("--")) throw new Error(`unexpected argument ${k}`);
    const v = argv[i + 1];
    if (v === undefined || v.startsWith("--")) out[k.slice(2)] = true;
    else {
      out[k.slice(2)] = v;
      i++;
    }
  }
  return out;
}

/** The bench's settings: flags over environment over defaults. */
export function settings(a = args()) {
  const repoRoot = a.repo ?? process.env.BENCH_REPO ?? REPO;
  return {
    rigDir: a["rig-dir"] ?? process.env.BENCH_RIG_DIR ?? join(repoRoot, "data", "avatar", "rig"),
    out: a.out ?? process.env.BENCH_OUT ?? join(repoRoot, "runs", "avatar-3d", "bench"),
    port: Number(a.port ?? process.env.BENCH_PORT ?? 5184),
    rigPort: Number(a["rig-port"] ?? process.env.BENCH_RIG_PORT ?? 5284),
    faces: String(a.faces ?? "cut,depth,relit,lich3d").split(","),
    sizes: String(a.sizes ?? "440,1024").split(",").map(Number),
    repeat: Number(a.repeat ?? 3),
    args: a,
  };
}

export function outDir(s, ...parts) {
  const d = join(s.out, ...parts);
  mkdirSync(d, { recursive: true });
  return d;
}

/** Playwright, from BENCH_PLAYWRIGHT (a directory with node_modules/playwright) or the resolver. */
export async function playwright() {
  const from = process.env.BENCH_PLAYWRIGHT;
  if (from) {
    const req = createRequire(join(resolve(from), "package.json"));
    const m = await import(pathToFileURL(req.resolve("playwright")).href);
    return m.chromium ? m : m.default;
  }
  try {
    const m = await import("playwright");
    return m.chromium ? m : m.default;
  } catch {
    throw new Error("Playwright is not installed for the bench: `npm i playwright` in a scratch directory and set BENCH_PLAYWRIGHT to it (README.md)");
  }
}

/** Chromium on the GPU (ANGLE D3D11 on Windows), with precise JS heap figures. */
export const GPU_ARGS = [
  "--use-angle=d3d11",
  "--enable-gpu",
  "--ignore-gpu-blocklist",
  "--enable-precise-memory-info",
  "--disable-features=LocalNetworkAccessChecks,PrivateNetworkAccessRespectPreflightResults",
];

/** A GPU Chromium as a server, so its process tree is known (`server.process().pid`), and a browser connected to it. */
export async function gpuBrowser() {
  const { chromium } = await playwright();
  const server = await chromium.launchServer({ channel: "chromium", args: GPU_ARGS });
  const browser = await chromium.connect(server.wsEndpoint());
  return {
    server,
    browser,
    pid: server.process().pid,
    async close() {
      await browser.close().catch(() => {});
      await server.close().catch(() => {});
    },
  };
}

/** A page at 1:1 device pixels, big enough that a 1024 px face is fully on screen. */
export async function benchPage(browser, log = () => {}) {
  const context = await browser.newContext({ viewport: { width: 1100, height: 1100 }, deviceScaleFactor: 1 });
  const page = await context.newPage();
  page.on("pageerror", (e) => log(`[pageerror] ${e.message}`));
  page.on("console", (m) => {
    const t = m.text();
    if (!/Failed to load resource|\[vite\]/.test(t)) log(`[page] ${t}`);
  });
  return { context, page };
}

const TYPES = { ".png": "image/png", ".json": "application/json", ".glb": "model/gltf-binary", ".npy": "application/octet-stream" };

/** Serve `dir` on 127.0.0.1:`port` with CORS (WebGL textures need it) and no caching. */
export function rigServer(dir, port) {
  if (!existsSync(join(dir, "layers.json"))) throw new Error(`no rig at ${dir} (layers.json)`);
  const server = createServer((req, res) => {
    const path = normalize(decodeURIComponent(new URL(req.url, "http://x").pathname)).replace(/^[\\/]+/, "");
    res.setHeader("Access-Control-Allow-Origin", "*");
    // so the page's resource timing may read the sizes (ready.mjs)
    res.setHeader("Timing-Allow-Origin", "*");
    res.setHeader("Cross-Origin-Resource-Policy", "cross-origin");
    res.setHeader("Cache-Control", "no-store");
    if (path.includes("..")) {
      res.writeHead(400);
      res.end();
      return;
    }
    try {
      const body = readFileSync(join(dir, path));
      res.writeHead(200, { "Content-Type": TYPES[extname(path)] ?? "application/octet-stream" });
      res.end(body);
    } catch {
      res.writeHead(404);
      res.end("not found");
    }
  });
  return new Promise((ok, fail) => {
    server.once("error", fail);
    server.listen(port, "127.0.0.1", () => ok({ url: `http://127.0.0.1:${port}/`, close: () => new Promise((r) => server.close(r)) }));
  });
}

const VITE = join(STAGE, "node_modules", "vite", "bin", "vite.js");

/** Wait until `url` answers. */
async function reachable(url, timeoutMs = 60_000) {
  const end = Date.now() + timeoutMs;
  while (Date.now() < end) {
    try {
      const r = await fetch(url);
      if (r.ok) return;
    } catch {
      // not up yet
    }
    await new Promise((r) => setTimeout(r, 250));
  }
  throw new Error(`${url} did not answer in ${timeoutMs} ms`);
}

function serveVite(argv, url) {
  const child = spawn(process.execPath, [VITE, ...argv], { cwd: STAGE, stdio: ["ignore", "pipe", "pipe"] });
  let out = "";
  child.stdout.on("data", (d) => (out += d));
  child.stderr.on("data", (d) => (out += d));
  return reachable(url).then(
    () => ({ url, close: () => kill(child) }),
    (e) => {
      kill(child);
      throw new Error(`${e.message}\n${out}`);
    },
  );
}

function kill(child) {
  if (child.exitCode !== null) return Promise.resolve();
  return new Promise((r) => {
    child.once("exit", r);
    // vite has no children of its own here; on Windows kill the tree anyway
    if (process.platform === "win32") spawnSync("taskkill", ["/pid", String(child.pid), "/t", "/f"], { stdio: "ignore" });
    else child.kill();
  });
}

/** The Vite dev server over the stage (bench/vite.config.mjs), the bench page at `<url>bench/index.html`. */
export function devServer(port) {
  return serveVite(["--config", join(BENCH, "vite.config.mjs"), "--port", String(port), "--strictPort", "--host", "127.0.0.1"], `http://127.0.0.1:${port}/bench/index.html`);
}

/** `vite build` of the bench page into `dist`, then `vite preview` of it. */
export async function previewServer(port, dist) {
  const b = spawnSync(process.execPath, [VITE, "build", "--config", join(BENCH, "vite.config.mjs"), "--outDir", dist, "--emptyOutDir"], { cwd: STAGE, encoding: "utf8" });
  if (b.status !== 0) throw new Error(`vite build failed:\n${b.stdout}${b.stderr}`);
  const s = await serveVite(["preview", "--config", join(BENCH, "vite.config.mjs"), "--outDir", dist, "--port", String(port), "--strictPort", "--host", "127.0.0.1"], `http://127.0.0.1:${port}/bench/index.html`);
  return { ...s, build: b.stdout };
}

/** The machine's busy share (all cores, percent) over the next `ms`. */
export async function machineLoad(ms = 3000) {
  const a = cpus();
  await new Promise((r) => setTimeout(r, ms));
  return machineBusy(a, cpus());
}

/** The processes using the most CPU right now (Windows: a 2 s sample), to name foreign load. */
export function topProcesses(n = 6) {
  if (process.platform !== "win32") return [];
  const ps = `$a = Get-Process | Select-Object Id, ProcessName, CPU; Start-Sleep -Milliseconds 2000; $b = Get-Process | Select-Object Id, ProcessName, CPU; $m = @{}; foreach ($p in $a) { $m[$p.Id] = $p.CPU }; $b | ForEach-Object { [pscustomobject]@{ name = $_.ProcessName; pid = $_.Id; pct = [math]::Round((($_.CPU - $m[$_.Id]) / 2) * 100, 1) } } | Where-Object { $_.pct -gt 0 } | Sort-Object pct -Descending | Select-Object -First ${n} | ConvertTo-Json -Compress`;
  const r = spawnSync("powershell", ["-NoProfile", "-Command", ps], { encoding: "utf8" });
  try {
    const v = JSON.parse(r.stdout.trim() || "[]");
    return Array.isArray(v) ? v : [v];
  } catch {
    return [];
  }
}

/** nvidia-smi now: name, utilisation %, memory used MiB, graphics clock MHz; undefined without it. */
export function gpuNow() {
  const r = spawnSync("nvidia-smi", ["--query-gpu=name,utilization.gpu,memory.used,clocks.gr", "--format=csv,noheader,nounits"], { encoding: "utf8" });
  if (r.status !== 0) return undefined;
  const [name, util, mem, clock] = r.stdout.trim().split("\n")[0].split(",").map((s) => s.trim());
  return { name, util: Number(util), memMiB: Number(mem), clockMHz: Number(clock) };
}

/** Sample nvidia-smi at 1 Hz until `stop()`; resolves to the rows. Undefined when there is no nvidia-smi. */
export function gpuSampler() {
  if (gpuNow() === undefined) return undefined;
  const child = spawn("nvidia-smi", ["--query-gpu=utilization.gpu,memory.used,clocks.gr", "--format=csv,noheader,nounits", "-lms", "1000"], { stdio: ["ignore", "pipe", "ignore"] });
  let buf = "";
  const rows = [];
  child.stdout.on("data", (d) => {
    buf += d;
    let k;
    while ((k = buf.indexOf("\n")) >= 0) {
      const line = buf.slice(0, k).trim();
      buf = buf.slice(k + 1);
      const [util, mem, clock] = line.split(",").map((s) => Number(s.trim()));
      if (Number.isFinite(util)) rows.push({ t: Date.now(), util, memMiB: mem, clockMHz: clock });
    }
  });
  return {
    async stop() {
      await kill(child);
      return rows;
    },
  };
}

/**
 * Sample every process's CPU time at 1 Hz (Windows: one CIM query a second, in one PowerShell)
 * until `stop()`; resolves to `[{t, procs: [{pid, ppid, cmd, cpuMs}]}]` for `names` (image names).
 */
export function procSampler(names = ["chrome.exe"]) {
  if (process.platform !== "win32") throw new Error("procSampler: Windows only (Win32_Process)");
  const filter = names.map((n) => `Name='${n}'`).join(" or ");
  const ps = `while ($true) { $t = [DateTimeOffset]::Now.ToUnixTimeMilliseconds(); $p = Get-CimInstance Win32_Process -Filter "${filter}" | ForEach-Object { @{ pid = $_.ProcessId; ppid = $_.ParentProcessId; cmd = $_.CommandLine; cpuMs = ($_.KernelModeTime + $_.UserModeTime) / 10000 } }; [Console]::Out.WriteLine((@{ t = $t; procs = @($p) } | ConvertTo-Json -Compress -Depth 4)); [Console]::Out.Flush(); $s = 1000 - ([DateTimeOffset]::Now.ToUnixTimeMilliseconds() - $t); if ($s -gt 0) { Start-Sleep -Milliseconds $s } }`;
  const child = spawn("powershell", ["-NoProfile", "-Command", ps], { stdio: ["ignore", "pipe", "ignore"] });
  let buf = "";
  const samples = [];
  child.stdout.on("data", (d) => {
    buf += d;
    let k;
    while ((k = buf.indexOf("\n")) >= 0) {
      const line = buf.slice(0, k).trim();
      buf = buf.slice(k + 1);
      try {
        samples.push(JSON.parse(line));
      } catch {
        // a partial line at stop
      }
    }
  });
  return {
    pid: child.pid,
    get count() {
      return samples.length;
    },
    async stop() {
      await kill(child);
      return samples;
    },
  };
}

/** Sample the machine's busy share (all cores, percent) once a second until `stop()`. */
export function loadSampler() {
  const rows = [];
  let prev = cpus();
  const timer = setInterval(() => {
    const now = cpus();
    rows.push({ t: Date.now(), busy: machineBusy(prev, now) });
    prev = now;
  }, 1000);
  return {
    stop() {
      clearInterval(timer);
      return rows;
    },
  };
}

export const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

/** The current commit, for the record. */
export function commit() {
  const r = spawnSync("git", ["rev-parse", "--short", "HEAD"], { cwd: STAGE, encoding: "utf8" });
  return r.status === 0 ? r.stdout.trim() : undefined;
}

/**
 * Open the bench page with `?auto=1`: the face starts at load on the page's own frame clock and the
 * bench script loops, as on the stage. Resolves once it is ready (or failed), with what it said.
 */
export async function liveFace(browser, pageUrl, { face, size, rig, bloom }, log = () => {}) {
  const { context, page } = await benchPage(browser, log);
  const q = new URLSearchParams({ auto: "1", face, size: String(size), rig });
  if (bloom !== undefined) q.set("bloom", bloom ? "1" : "0");
  await page.goto(`${pageUrl}?${q}`);
  await page.waitForFunction(() => !!window.__benchReady);
  const ready = await page.evaluate(() => window.__benchReady);
  return { context, page, ready };
}

/** Configs for faces x sizes, lich3d also without bloom (`withNoBloom`). */
export function faceConfigs(faces, sizes, withNoBloom = true) {
  const out = [];
  for (const face of faces) {
    for (const size of sizes) {
      out.push({ face, size });
      if (face === "lich3d" && withNoBloom) out.push({ face, size, bloom: false });
    }
  }
  return out;
}

export const configLabel = (c) => `${c.face}${c.size ? `-${c.size}` : ""}${c.bloom === false ? "-nobloom" : ""}`;
