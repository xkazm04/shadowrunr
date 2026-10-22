// Tallies builder returns per cohort; tolerates an unescaped backslash in a builder's note.
const fs = require("fs");
const parse = (f) => JSON.parse(fs.readFileSync(`returns/${f}`, "utf8").replace(/\\(?!["\\/bfnrtu])/g, "\\\\"));
const cohorts = process.argv.slice(2).map((s) => s.split(","));
for (const c of cohorts) {
  const r = fs.readdirSync("returns").filter((f) => c.some((x) => f.startsWith(`${x}-`))).map(parse);
  const sum = (k) => r.reduce((s, x) => s + (x[k] || 0), 0);
  console.log(c[0], { n: r.length, written: sum("cases_total"), red: sum("cases_red_before"), green: sum("cases_green_after"), lines: sum("lines_changed") });
}
const all = fs.readdirSync("returns").filter((f) => !f.startsWith("finisher")).map(parse);
console.log("returns", all.length, "landed", all.filter((x) => x.status === "landed").length,
  "cases", all.reduce((s, x) => s + (x.cases_total || 0), 0), "green", all.reduce((s, x) => s + (x.cases_green_after || 0), 0));
