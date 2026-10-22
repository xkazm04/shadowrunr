// Greedy wave packing per cohort: disjoint write sets, contract regenerators never share a wave,
// slot A before slot B of the same context when their write sets overlap.
const fs = require("fs");
const cohort = JSON.parse(fs.readFileSync("cohort.json", "utf8"));
const GEN = /packages\/contract\/(schema|src\/generated)\//;
const out = [];
for (const run of cohort.runs.slice(1)) {
  const cards = [];
  for (const ctx of run.hosts) for (const c of JSON.parse(fs.readFileSync(`cards/${ctx}.json`, "utf8"))) {
    if (c.critic?.verdict === "void") continue;
    const ws = new Set([...(c.write_set || []), ...(c.new_files || [])].map((p) => p.split("\\").join("/")));
    const gen = [...ws].some((p) => GEN.test(p)) || /contract/i.test(JSON.stringify(c.shared_surfaces || []));
    cards.push({ id: `${ctx}:${c.slot[0]}`, ctx, slot: c.slot[0], ws, gen });
  }
  const left = [...cards];
  const waves = [];
  while (left.length) {
    const wave = [];
    for (const c of [...left]) {
      if (wave.length >= 4) break;
      if (wave.some((w) => [...w.ws].some((p) => c.ws.has(p)))) continue;
      if (c.gen && wave.some((w) => w.gen)) continue;
      const a = left.find((x) => x.ctx === c.ctx && x.slot === "A" && x !== c);
      if (c.slot === "B" && a && [...a.ws].some((p) => c.ws.has(p))) continue;
      wave.push(c);
      left.splice(left.indexOf(c), 1);
    }
    if (!wave.length) wave.push(left.shift());
    waves.push(wave.map((w) => w.id + (w.gen ? "*" : "")));
  }
  out.push({ n: run.n, waves });
}
console.log(JSON.stringify(out));
