// The fidelity gate (contest promotion steps 2, 3 and 5): `pnpm -C apps/console fidelity`.
//
//   node fidelity/run.mjs                 check every page against its captured contract
//   node fidelity/run.mjs --page segments check one page
//   node fidelity/run.mjs capture         re-capture the contracts from the prototypes
//
// It starts the harness (fidelity/serve.mjs: the console's own Vite dev server rendering the real
// Layer1 components on the contest's real data), runs style-contract.py per page and texture.py,
// and exits non-zero on any deviation. In check mode each contract is first merged with the owner's
// exceptions (fidelity/exceptions.json, exceptions.mjs) into a copy in the run's tmp dir, and the
// summary names every exception it honoured; an exception that no longer matches its contract
// fails the run, offline, before any prerequisite is probed. Capture mode writes the winner as
// captured and then reports any exception whose winner value moved under it. A local gate: it needs
// Python with Playwright, the linked contest skill and the git-ignored contest data, so CI does not
// run it. A missing prerequisite prints "skipped: <reason>" and exits 0, unless FIDELITY_REQUIRED=1.
//
// Environment: SHADOWRUNR_CONTEST_DATA (required: the contest's shadowrunr-data.js; the prototypes
// are found beside it), CONTEST_SKILL_DIR (default <repo>/.claude/skills/contest), PYTHON
// (default python), FIDELITY_REQUIRED.
import { spawn, spawnSync } from "node:child_process";
import { existsSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { createServer } from "node:http";
import { tmpdir } from "node:os";
import { dirname, extname, join, normalize, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";
import { contractFile, describeApplied, loadExceptions } from "./exceptions.mjs";
import { contestDataPath, startHarness } from "./serve.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const repo = resolve(here, "../../..");
const args = process.argv.slice(2);
const mode = args[0] === "capture" ? "capture" : "check";
const only = args.includes("--page") ? args[args.indexOf("--page") + 1] : undefined;
const python = process.env.PYTHON || "python";
const skill = process.env.CONTEST_SKILL_DIR || join(repo, ".claude", "skills", "contest");
const tool = join(skill, "scripts", "style-contract.py");
const spec = JSON.parse(readFileSync(join(here, "roles.json"), "utf8"));

function skip(reason) {
  const required = process.env.FIDELITY_REQUIRED === "1";
  console.log(`fidelity: skipped: ${reason}${required ? " (FIDELITY_REQUIRED=1, so this fails)" : ""}`);
  process.exit(required ? 1 : 0);
}

/** Fails the run on any exception problem: an owner exception is applied or reported, never dropped. */
function reportProblems(problems) {
  if (problems.length === 0) return false;
  console.log(`fidelity: ${problems.length} owner exception problem(s) (fidelity/exceptions.json):\n  ${problems.join("\n  ")}`);
  return true;
}

const pageIds = Object.keys(spec.pages).filter((id) => only === undefined || id === only || id === `${only}-a1`);
const runTargets = only === undefined ? [...pageIds, "texture"] : pageIds;
const overlay = mode === "check" ? loadExceptions(here, runTargets) : null;
if (overlay && reportProblems(overlay.problems)) process.exit(1);

const data = contestDataPath();
if (data.error) skip(data.error);
if (!existsSync(tool)) skip(`style-contract.py not found at ${tool} (link the contest skill or set CONTEST_SKILL_DIR)`);
const probe = spawnSync(python, ["-c", "import playwright.sync_api"], { encoding: "utf8" });
if (probe.status !== 0) skip(`${python} cannot import playwright (pip install playwright && playwright install chromium)`);

const pages = Object.entries(spec.pages).filter(([id]) => pageIds.includes(id));
if (pages.length === 0) skip(`no page named ${only}`);

/** Serves the contest arena (the folder two levels above the data file) for the prototypes. */
function serveArena(root) {
  const types = { ".html": "text/html", ".js": "text/javascript", ".css": "text/css", ".json": "application/json", ".svg": "image/svg+xml" };
  const server = createServer((req, res) => {
    const path = normalize(join(root, decodeURIComponent(new URL(req.url ?? "/", "http://x").pathname)));
    if (!path.startsWith(root + sep) || !existsSync(path)) {
      res.writeHead(404).end();
      return;
    }
    res.writeHead(200, { "content-type": types[extname(path)] ?? "application/octet-stream" }).end(readFileSync(path));
  });
  return new Promise((ok) => server.listen(0, "127.0.0.1", () => ok({ port: server.address().port, close: () => server.close() })));
}

/** Runs a child without blocking this process, which serves the harness and the prototypes. */
function run(cmd, argv) {
  return new Promise((ok) => {
    const child = spawn(cmd, argv, { cwd: here });
    let out = "";
    child.stdout.on("data", (d) => (out += d));
    child.stderr.on("data", (d) => (out += d));
    child.on("close", (code) => ok({ ok: code === 0, out }));
  });
}

const tmp = mkdtempSync(join(tmpdir(), "sr-fidelity-"));
/** The contract a check holds the port to: the captured file, or its merge with the exceptions. */
function contractFor(target) {
  const captured = join(here, contractFile(target));
  const merged = overlay?.merged.get(target);
  if (merged === undefined) return captured;
  const copy = join(tmp, contractFile(target));
  writeFileSync(copy, JSON.stringify(merged, null, 2) + "\n");
  return copy;
}
const summary = [];
let failed = false;
const harness = await startHarness();
const arena = mode === "capture" ? await serveArena(resolve(dirname(data.path), "..", "..")) : null;
try {
  for (const [id, page] of pages) {
    const roles = Object.fromEntries(Object.entries(spec.roles).filter(([k]) => k.startsWith(`${id}.`)));
    const rolesFile = join(tmp, `roles.${id}.json`);
    writeFileSync(rolesFile, JSON.stringify({ roles }));
    const contract = contractFor(id);
    const drive = (mode === "capture" ? page.winnerDrive : page.portDrive) ?? [];
    const width = mode === "capture" ? page.viewport : (page.portViewport ?? page.viewport);
    const target =
      mode === "capture"
        ? `http://127.0.0.1:${arena.port}/${spec.sources[page.source]}${page.route}`
        : `${harness.url}${page.portQuery ? `?${page.portQuery}` : ""}${page.route}`;
    const r = await run(python, [tool, mode, target, rolesFile, contract, "--width", width, ...drive.flatMap((d) => ["--drive", d])]);
    const last = r.out.trim().split("\n").filter(Boolean).at(-1) ?? "";
    process.stdout.write(`\n== ${id} (${page.source}, ${width})\n${r.out.trim()}\n`);
    summary.push(`${id}: ${last}`);
    if (!r.ok) failed = true;
  }
  if (only === undefined) {
    const texture = contractFor("texture");
    const target = mode === "capture" ? `http://127.0.0.1:${arena.port}/${spec.sources.winner}` : harness.url;
    const r = await run(python, [join(here, "texture.py"), mode, target, join(here, "roles.json"), texture, "--width", spec.pages.characters.viewport]);
    process.stdout.write(`\n== texture\n${r.out.trim()}\n`);
    summary.push(`texture: ${r.out.trim().split("\n").filter(Boolean).at(-1) ?? ""}`);
    if (!r.ok) failed = true;
  }
  if (overlay) summary.push(describeApplied(overlay.applied));
  if (mode === "capture" && reportProblems(loadExceptions(here, runTargets).problems)) failed = true;
} finally {
  await harness.close();
  arena?.close();
  rmSync(tmp, { recursive: true, force: true });
}
console.log(`\nfidelity ${mode}:\n  ${summary.join("\n  ")}`);
process.exit(failed ? 1 : 0);
