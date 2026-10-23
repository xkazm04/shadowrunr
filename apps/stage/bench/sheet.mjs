// The expressiveness sheet: rows = channels (rest, yaw and pitch at a full look, jaw open, eye light
// 0.3 and 2.5, brow -1 and +1, flare and lean_in at their peaks), columns = faces, each face's
// renderer held at exactly the same values (sheet.js). Dev server, GPU Chromium.
//
//   node bench/sheet.mjs [--faces cut,depth,relit,lich3d] [--size 440]
//
// Writes <out>/sheet.png and <out>/sheet.json (the rows and each face's head-turn limits).

import { writeFileSync } from "node:fs";
import { join } from "node:path";

import { benchPage, devServer, gpuBrowser, outDir, rigServer, settings } from "./lib/env.mjs";

const s = settings();
outDir(s);
const size = Number(s.args.size ?? 440);
const rig = await rigServer(s.rigDir, s.rigPort);
const web = await devServer(s.port);
let gpu = null;
try {
  gpu = await gpuBrowser();
  const { page } = await benchPage(gpu.browser, (m) => console.log(`  ${m}`));
  await page.goto(web.url);
  await page.waitForFunction(() => !!window.__bench);
  const r = await page.evaluate((o) => window.__bench.sheet(o), { rig: rig.url, size, order: s.faces });
  writeFileSync(join(s.out, "sheet.png"), Buffer.from(r.url.split(",")[1], "base64"));
  writeFileSync(join(s.out, "sheet.json"), JSON.stringify({ size, faces: s.faces, rows: r.rows, limitsDeg: r.limits }, null, 1));
  console.log(`wrote ${join(s.out, "sheet.png")}`);
} finally {
  await gpu?.close();
  await web.close();
  await rig.close();
}
