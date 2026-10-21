// Owner exceptions to the fidelity contracts: `node fidelity/exceptions.mjs [--dir <folder>]`.
//
// A contract.<target>.json is a capture of the winning prototype and means "what the winner is";
// it is never hand-edited. Where the owner decided the port should differ from the winner, the
// difference is one entry in exceptions.json - target (a contract.<target>.json), key, prop, the
// winner value it overrides, the port value, why, the decision date and a ref - applied to a copy
// of the contract at check time (run.mjs). An entry is honoured only while the captured winner
// still holds the value it names: a recapture that moved the winner, a key or prop the contract
// does not have, a duplicate, or an entry missing its reason or date is a problem, which fails the
// gate. Nothing is dropped silently.
//
// Run as a script it is the offline half of the fidelity gate (no Playwright, no contest data):
// it checks every exception against the committed contracts and prints what it would waive.
import { existsSync, readFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const REQUIRED = ["target", "key", "prop", "winner", "port", "why", "decided", "ref"];
const DATE = /^\d{4}-\d{2}-\d{2}$/;

/** The contract file an exception's target names. */
export const contractFile = (target) => `contract.${target}.json`;

const id = (e) => `${e.target} ${e.key}.${e.prop}`;

/**
 * Parses exceptions.json (text or the parsed object). An entry missing a field, with a blank
 * string, or with a `decided` that is not YYYY-MM-DD is unparseable and is not applied; a second
 * entry on the same target/key/prop is a duplicate and is not applied.
 * @returns {{ exceptions: object[], problems: string[] }}
 */
export function parseExceptions(json) {
  const doc = typeof json === "string" ? JSON.parse(json) : json;
  const problems = [];
  const exceptions = [];
  if (!doc || !Array.isArray(doc.exceptions)) return { exceptions, problems: ["exceptions.json: unparseable: no `exceptions` array"] };
  const seen = new Set();
  doc.exceptions.forEach((e, i) => {
    const bad = REQUIRED.filter((f) => typeof e?.[f] !== "string" || e[f].trim() === "");
    if (bad.length === 0 && !DATE.test(e.decided)) bad.push("decided (not YYYY-MM-DD)");
    if (bad.length > 0) {
      problems.push(`exception ${i + 1}${e?.key ? ` (${e.key})` : ""}: unparseable: missing or empty ${bad.join(", ")}`);
      return;
    }
    if (seen.has(id(e))) {
      problems.push(`${id(e)}: duplicate exception`);
      return;
    }
    seen.add(id(e));
    exceptions.push(e);
  });
  return { exceptions, problems };
}

/** The record a key names: a page contract's flat `page.role`, or a texture's `page.layer`. */
function locate(contract, key) {
  if (Object.hasOwn(contract, key)) return contract[key];
  const dot = key.indexOf(".");
  if (dot < 0) return undefined;
  const outer = contract[key.slice(0, dot)];
  const inner = outer && typeof outer === "object" ? outer[key.slice(dot + 1)] : undefined;
  return inner && typeof inner === "object" ? inner : undefined;
}

/**
 * Applies the exceptions for one target to a deep copy of its contract.
 * @returns {{ contract: object, applied: object[], problems: string[] }}
 */
export function applyExceptions(contract, exceptions, target) {
  const out = structuredClone(contract);
  const applied = [];
  const problems = [];
  for (const e of exceptions) {
    if (e.target !== target) continue;
    const rec = locate(out, e.key);
    if (!rec || !Object.hasOwn(rec, e.prop)) {
      problems.push(`${e.key}.${e.prop}: matches nothing in ${contractFile(target)}`);
      continue;
    }
    if (rec[e.prop] !== e.winner) {
      problems.push(`${e.key}.${e.prop}: the winner moved from ${e.winner} to ${rec[e.prop]} under an owner exception; re-decide it`);
      continue;
    }
    rec[e.prop] = e.port;
    applied.push(e);
  }
  return { contract: out, applied, problems };
}

/** The summary line a green run prints, naming every difference it waived. */
export function describeApplied(applied) {
  const lines = [`${applied.length} owner exception(s) applied`];
  for (const e of applied) lines.push(`  ${id(e)}: winner ${e.winner} -> port ${e.port} (${e.decided}, ${e.ref})`);
  return lines.join("\n");
}

/**
 * Loads exceptions.json from a fidelity folder and checks each exception against its contract.
 * `targets` limits which contracts are merged (default: every target an exception names).
 * @returns {{ merged: Map<string, object>, applied: object[], problems: string[] }}
 */
export function loadExceptions(dir, targets) {
  const file = join(dir, "exceptions.json");
  const merged = new Map();
  if (!existsSync(file)) return { merged, applied: [], problems: [] };
  let parsed;
  try {
    parsed = parseExceptions(readFileSync(file, "utf8"));
  } catch (err) {
    return { merged, applied: [], problems: [`exceptions.json: unparseable: ${err.message}`] };
  }
  const { exceptions } = parsed;
  const problems = [...parsed.problems];
  const applied = [];
  const wanted = targets ?? [...new Set(exceptions.map((e) => e.target))];
  for (const target of new Set([...wanted, ...exceptions.map((e) => e.target)])) {
    const path = join(dir, contractFile(target));
    if (!existsSync(path)) {
      for (const e of exceptions.filter((x) => x.target === target)) problems.push(`${e.key}.${e.prop}: matches nothing: there is no ${contractFile(target)}`);
      continue;
    }
    const r = applyExceptions(JSON.parse(readFileSync(path, "utf8")), exceptions, target);
    if (wanted.includes(target)) merged.set(target, r.contract);
    applied.push(...(wanted.includes(target) ? r.applied : []));
    problems.push(...r.problems);
  }
  return { merged, applied, problems };
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const argv = process.argv.slice(2);
  const dir = argv.includes("--dir") ? resolve(argv[argv.indexOf("--dir") + 1]) : dirname(fileURLToPath(import.meta.url));
  const { applied, problems } = loadExceptions(dir);
  console.log(describeApplied(applied));
  if (problems.length > 0) {
    console.log(`\n${problems.length} owner exception problem(s):\n  ${problems.join("\n  ")}`);
    process.exit(1);
  }
}
