// Starts the console's own Vite dev server (its vite.config.ts, so the same CSS Modules, tokens
// and plugins) with one extra dev-only module: the contest data the harness renders.
//
//   SHADOWRUNR_CONTEST_DATA=<path to shadowrunr-data.js> node fidelity/serve.mjs [port]
//
// The harness is then at http://127.0.0.1:<port>/fidelity/harness.html#/<page>.
// The contest folder is git-ignored and local, so its path only ever comes from the environment.
import { existsSync, readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
export const CONSOLE_ROOT = resolve(here, "..");
export const DATA_ENV = "SHADOWRUNR_CONTEST_DATA";
const VIRTUAL = "virtual:shadowrunr-contest-data";

/** The contest data path from the environment, or why it cannot be used. */
export function contestDataPath() {
  const p = process.env[DATA_ENV];
  if (!p) return { error: `${DATA_ENV} is not set (the path to the contest's shadowrunr-data.js)` };
  if (!existsSync(p)) return { error: `${DATA_ENV} points at ${p}, which does not exist` };
  return { path: resolve(p) };
}

function contestData(path) {
  return {
    name: "shadowrunr-contest-data",
    apply: "serve",
    resolveId: (id) => (id === VIRTUAL ? `\0${VIRTUAL}` : null),
    load(id) {
      if (id !== `\0${VIRTUAL}`) return null;
      // The prototypes load it as a script that assigns one global; as a module it is the default export.
      const src = readFileSync(path, "utf8");
      const body = src.replace(/^\s*window\.SHADOWRUNR_DATA\s*=\s*/, "");
      if (body === src) throw new Error(`${path} does not start with window.SHADOWRUNR_DATA =`);
      return `export default ${body}`;
    },
  };
}

/** Starts the harness server; resolves to { url, close }. */
export async function startHarness(port = 0) {
  const data = contestDataPath();
  if (data.error) throw new Error(data.error);
  const vite = await import("vite");
  const server = await vite.createServer({
    root: CONSOLE_ROOT,
    configFile: resolve(CONSOLE_ROOT, "vite.config.ts"),
    plugins: [contestData(data.path)],
    server: { port, strictPort: false, host: "127.0.0.1", hmr: false },
    clearScreen: false,
    logLevel: "warn",
  });
  await server.listen();
  const addr = server.httpServer?.address();
  const actual = typeof addr === "object" && addr ? addr.port : port;
  return { url: `http://127.0.0.1:${actual}/fidelity/harness.html`, close: () => server.close() };
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const h = await startHarness(Number(process.argv[2] ?? 0));
  console.log(`harness: ${h.url}#/characters  (Ctrl+C stops it)`);
}
