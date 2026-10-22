// Close-out appends: one registry lead, one consult line, and the outbox progress nodes.
const fs = require("fs");
const ts = new Date().toISOString();
fs.appendFileSync(".ai/registry-leads.jsonl", JSON.stringify({
  ts, bundle: "software-engineering", nearest: "test-harness", kind: "technique",
  claim: "When a test polls a log file that concurrently running fake child processes append to, have each fake emit a record in one write call and have the reader parse only newline-terminated lines, because formatted writes can split a record and a mid-write read fails as a load-only flake.",
  because: "Two load-only flakes in one run had this cause: a Python fake worker lost pid lines (1/15 -> 30/30 after a locked whole-record append, seeded red 20/20) and a Rust fake CLI's writeln! split JSONL records read mid-write ('EOF while parsing an object'); both fixed and green.",
  confidence: "medium", from: "scan-sweep@challenge",
}) + "\n");
fs.appendFileSync(".ai/consults.jsonl", JSON.stringify({
  ts, bundle: "software-engineering", subjects: ["ipc-contract", "table", "quality-gates", "talking-avatar", "voice-io", "agent-runtime-assembly", "eval-harness"],
  techniques: [], deviations: 3,
}) + "\n");
fs.appendFileSync(".ai/consults.jsonl", JSON.stringify({
  ts, bundle: "media-generation", subjects: ["live-safety-rails", "stream-moment-capture", "simulated-audience-testing", "generated-speech-acceptance"],
  techniques: [], deviations: 1,
}) + "\n");

const parse = (f) => JSON.parse(fs.readFileSync(`.claude/scan-sweep/challenge-2026-10-21/returns/${f}`, "utf8").replace(/\\(?!["\\/bfnrtu])/g, "\\\\"));
const cards = {};
for (const f of fs.readdirSync(".claude/scan-sweep/challenge-2026-10-21/cards").filter((f) => !f.includes("riders"))) {
  for (const c of JSON.parse(fs.readFileSync(`.claude/scan-sweep/challenge-2026-10-21/cards/${f}`, "utf8"))) cards[`${c.context}-${c.slot[0]}`] = c;
}
const lines = [];
for (const f of fs.readdirSync(".claude/scan-sweep/challenge-2026-10-21/returns").filter((f) => !f.startsWith("finisher"))) {
  const r = parse(f);
  const c = cards[f.replace(/\.json$/, "")];
  if (!c || r.status !== "landed") continue;
  lines.push({ type: "node", kind: "progress", skill: `scan-${c.lens}`, context: c.context, title: `Fixed (challenge): ${c.title}`,
    body: `${(r.shas || []).join(",")}; ${r.cases_green_after}/${r.cases_total} acceptance cases green (${r.cases_red_before} red before)` });
}
const contexts = [...new Set(Object.values(cards).map((c) => c.context))];
for (const ctx of contexts) {
  const built = lines.filter((l) => l.context === ctx).length;
  lines.push({ type: "node", kind: "progress", skill: "scan-sweep", context: ctx, title: `Sweep of ${ctx}`, body: `challenge: 2 lenses; ${built} built, 0 backlogged, 0 escalations, 0 leads` });
}
fs.appendFileSync(".personas/memory-outbox.jsonl", lines.map((l) => JSON.stringify(l)).join("\n") + "\n");
console.log("outbox +", lines.length);
