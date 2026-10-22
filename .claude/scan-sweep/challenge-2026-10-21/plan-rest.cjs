// Packs every approved, not-yet-landed card into waves: disjoint write sets, at most one
// contract regenerator per wave, slot A of a context before its slot B, cohort order kept.
const fs = require("fs");
const cohort = JSON.parse(fs.readFileSync("cohort.json", "utf8"));
const GEN = /packages\/contract\/(schema|src\/generated)\//;
const landed = new Set(fs.readdirSync("returns").map((f) => f.replace(/\.json$/, "")));
const cards = [];
for (const run of cohort.runs) for (const ctx of run.hosts) {
  for (const c of JSON.parse(fs.readFileSync(`cards/${ctx}.json`, "utf8"))) {
    const id = `${ctx}-${c.slot[0]}`;
    if (c.critic?.verdict === "void" || landed.has(id)) continue;
    const ws = new Set([...(c.write_set || []), ...(c.new_files || [])].map((p) => p.split("\\").join("/")));
    const gen = [...ws].some((p) => GEN.test(p)) || /contract/i.test(JSON.stringify(c.shared_surfaces || []));
    cards.push({ id, ctx, slot: c.slot[0], ws, gen, n: run.n });
  }
}
const left = [...cards];
const waves = [];
while (left.length) {
  const wave = [];
  for (const c of [...left]) {
    if (wave.length >= 4) break;
    if (wave.some((w) => [...w.ws].some((p) => c.ws.has(p)))) continue;
    if (c.gen && wave.some((w) => w.gen)) continue;
    if (c.slot === "B" && left.some((x) => x.ctx === c.ctx && x.slot === "A" && x !== c)) continue;
    wave.push(c);
    left.splice(left.indexOf(c), 1);
  }
  if (!wave.length) wave.push(left.shift());
  waves.push(wave.map((w) => `${w.id}${w.gen ? "*" : ""}(c${w.n})`));
}
waves.forEach((w, i) => console.log(`w${i + 1}: ${w.join(", ")}`));
