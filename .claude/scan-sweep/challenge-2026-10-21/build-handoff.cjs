// Builds the §9.1 hand-off: builder-found follow-ups, excluded void cards (Lane C),
// and floor-failed void cards re-homed as ordinary Lane B items.
const fs = require("fs");
const src = "challenge-2026-10-21";
const out = [];
const body = (summary, description, flow, impact, ev) =>
  `## Summary\n${summary}\n\n## Description\n${description}\n\n## Flow\n${flow.map((f) => `- ${f}`).join("\n")}\n\n## Expected impact\n${impact}\n\n## Evaluation\n${ev}`;

for (const line of fs.readFileSync("followups.jsonl", "utf8").trim().split("\n")) {
  const f = JSON.parse(line);
  const lane = f.lane;
  const gate = f.gate || "none";
  out.push({
    type: "finding", skill: "scan-sweep", lens: "architecture-challenger", context: f.context, title: f.title,
    body: body(
      f.title + ".",
      `Found by a challenge builder (${f.source}) while landing an approved card in run ${src}; outside that card's write set, so it was recorded rather than built. Evidence: ${f.evidence}.`,
      ["re-verify the cited evidence at HEAD", "write a failing test that pins the gap", "fix inside the owning context"],
      "Closes a gap a landed challenge card exposed; the risk is limited to the owning context.",
      `Claim: quality - ${f.title}\nBefore: observed by the builder (see evidence)\nAfter: unmeasured - no build was attempted\nMethod: simulation - seen while building an adjacent card; the gate rung needs its own test\nResult: unmeasurable\nGate: ${gate}`),
    evidence: f.evidence, size: "S", effort: 3, impact: 6, risk: 3,
    result: "unmeasurable", method: "simulation", gate, disposition: "backlog", lane, source: src,
    instrument: lane === "B" ? "a failing test in the owning context, then the fix" : null,
  });
}

const rehome = {
  "stage-app:A": { lane: "B", gate: "none", note: "Critic: real but S-sized (two-line drift plus a golden test). Re-verify at HEAD: another session changed seg-02a.toml after the card was written." },
  "voice-sidecar:B": { lane: "B", gate: "contract", note: "Critic: an honest S (additive hello/audio fields plus a parse branch); the daemon still matches the hello's reason text." },
  "console-segments-page:A": { lane: "B", gate: "none", note: "Restated stage-kit-scene-engine A; the residue is S: the console vote calculator should call stage-kit leaders() instead of its own tie rule." },
  "reconcile-engine:B": { lane: "C", gate: "policy-loosen", note: "Excluded from challenge mode: first runtime write path of model-composed, chat-derived text into the owner's tracked bible.md/tones, which feed the trusted prompt. Needs the owner by name." },
  "daemon-integrations:B": { lane: "C", gate: "contract", note: "Excluded: widens the Personas webhook payload (a contract another repository consumes) and fires the persona's trigger." },
};
for (const file of fs.readdirSync("cards").filter((f) => !f.includes("riders"))) {
  for (const c of JSON.parse(fs.readFileSync(`cards/${file}`, "utf8"))) {
    const key = `${c.context}:${c.slot[0]}`;
    if (c.critic?.verdict !== "void" || !rehome[key]) continue;
    const r = rehome[key];
    out.push({
      type: "finding", skill: "scan-sweep", lens: c.lens, context: c.context, title: c.title,
      body: c.body.replace(/\nGate: .*$/m, `\nGate: ${r.gate}`) + `\n\nRouting note (critic, run ${src}): ${r.note}`,
      evidence: c.evidence, size: r.lane === "C" ? c.size : "S", effort: r.lane === "C" ? c.effort : 3,
      impact: c.impact, risk: c.risk, result: r.lane === "C" ? "unmeasurable" : "better",
      method: "simulation", gate: r.gate, disposition: "backlog", lane: r.lane, source: src,
      instrument: r.lane === "B" ? "the card's own acceptance cases, trimmed to its S core" : null,
    });
  }
}
fs.writeFileSync(`../handoff-${src}.jsonl`, out.map((o) => JSON.stringify(o)).join("\n") + "\n");
console.log(out.length, out.map((o) => `${o.lane} ${o.context}`).join("; "));
