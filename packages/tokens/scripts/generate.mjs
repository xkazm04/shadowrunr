#!/usr/bin/env node
// Generates src/tokens.css and src/tokens.ts from tokens.source.json (primitives -> roles).
//   node packages/tokens/scripts/generate.mjs           write both files
//   node packages/tokens/scripts/generate.mjs --check   exit 1 when either file drifts from the source
// Exit 2 when the source holds no roles (the gate walked nothing).

import { existsSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

import source from "../tokens.source.json" with { type: "json" };

const PKG = join(dirname(fileURLToPath(import.meta.url)), "..");
const COMMAND = "node packages/tokens/scripts/generate.mjs";
const OUT_CSS = "src/tokens.css";
const OUT_TS = "src/tokens.ts";

/** CSS custom-property prefix per role group. */
const PREFIX = {
  color: "--sr-color-",
  alpha: "--sr-color-",
  font: "--sr-font-",
  weight: "--sr-weight-",
  size: "--sr-size-",
  root: "--sr-root-",
  space: "--sr-space-",
  stroke: "--sr-stroke-",
  radius: "--sr-radius-",
  z: "--sr-z-",
  ease: "--sr-ease-",
  breakpoint: "--sr-bp-",
  texture: "--sr-",
  layout: "--sr-layout-",
  // A/1's canvas palettes (fidelity roles, kept exact): the brain scope and the simulator grid.
  brain: "--sr-brain-",
  sim: "--sr-sim-",
};

const errors = [];
const used = new Set();

function resolve(ref, where) {
  const m = /^\{([a-z]+)\.([^}]+)\}$/.exec(ref);
  if (!m) {
    errors.push(`${where}: "${ref}" is not a {group.name} reference`);
    return "";
  }
  const [, group, name] = m;
  const value = source.primitive[group]?.[name];
  if (value === undefined) {
    errors.push(`${where}: ${ref} does not resolve to a primitive`);
    return "";
  }
  used.add(`${group}.${name}`);
  return value;
}

function hexChannels(hex, where) {
  const m = /^#([0-9a-f]{3}|[0-9a-f]{6})$/.exec(hex);
  if (!m) {
    errors.push(`${where}: ${hex} is not a lowercase #rgb or #rrggbb colour`);
    return [0, 0, 0];
  }
  const h = m[1].length === 3 ? [...m[1]].map((c) => c + c).join("") : m[1];
  return [0, 2, 4].map((i) => Number.parseInt(h.slice(i, i + 2), 16));
}

function renderRole(group, name, role) {
  const where = `role.${group}.${name}`;
  if (typeof role.description !== "string" || role.description.length === 0) {
    errors.push(`${where}: missing its one-sentence description`);
  }
  if (Array.isArray(role.value)) return role.value.map((r) => resolve(r, where)).join(",");
  const value = resolve(role.value, where);
  // A role with an `alpha` is that primitive at that alpha (the alpha group, and the canvas
  // palettes' translucent inks), written exactly as the winner and A/1 wrote it.
  if (group === "alpha" || role.alpha !== undefined) return `rgba(${hexChannels(value, where).join(",")},${role.alpha})`;
  return value;
}

const camel = (s) => s.replace(/-([a-z0-9])/g, (_, c) => c.toUpperCase());
const kebab = (s) => s.replace(/_/g, "-").toLowerCase();
/** Inverse of camel, mirrored by cssVar() in src/index.ts. */
const unCamel = (s) => s.replace(/[A-Z]/g, (c) => `-${c.toLowerCase()}`);

// ---- resolve everything once -------------------------------------------------------------

const roles = {}; // group -> [{ name, value, cssVar }]
const overrides = []; // { breakpoint, cssVar, value }
let roleCount = 0;
for (const [group, entries] of Object.entries(source.role)) {
  if (!PREFIX[group]) errors.push(`role.${group}: no CSS prefix is declared for this group`);
  roles[group] = [];
  for (const [name, role] of Object.entries(entries)) {
    const cssVar = `${PREFIX[group] ?? "--sr-"}${name}`;
    if (unCamel(camel(name)) !== name) errors.push(`role.${group}.${name}: the name does not survive camelCase and back`);
    roles[group].push({ name, value: renderRole(group, name, role), cssVar });
    roleCount += 1;
    for (const [bp, ref] of Object.entries(role.at ?? {})) {
      overrides.push({ breakpoint: bp, cssVar, value: resolve(ref, `role.${group}.${name}.at.${bp}`) });
    }
  }
}

const pageRoles = Object.keys(source.pageRole);
const pages = Object.entries(source.page).map(([id, palette]) => {
  for (const r of pageRoles) if (!(r in palette)) errors.push(`page.${id}: missing page role "${r}"`);
  for (const r of Object.keys(palette)) if (!pageRoles.includes(r)) errors.push(`page.${id}: unknown page role "${r}"`);
  return { id, values: pageRoles.map((r) => ({ role: r, value: resolve(palette[r] ?? "", `page.${id}.${r}`) })) };
});

const keyed = (block, label) =>
  Object.entries(block)
    .filter(([k]) => !k.startsWith("$"))
    .map(([key, ref]) => ({ key, value: resolve(ref, `${label}.${key}`) }));
const ranks = keyed(source.rank, "rank");
const grades = keyed(source.grade, "grade");

const ff = source.fontFace;
const fontFamily = resolve(ff.family, "fontFace.family");
const fontFaces = ff.weights.flatMap((wRef) => {
  const weight = resolve(wRef, "fontFace.weights");
  return Object.entries(ff.subsets).map(([subset, range]) => {
    const file = ff.file.replace("{subset}", subset).replace("{weight}", weight);
    if (!existsSync(join(PKG, file))) errors.push(`fontFace: ${file} is missing`);
    return { weight, subset, range, file };
  });
});

const breakpoints = roles.breakpoint ?? [];
const bpPx = (name) => {
  const bp = breakpoints.find((b) => b.name === name);
  if (!bp) errors.push(`breakpoint "${name}" is referenced but not declared`);
  return bp ? Number.parseInt(bp.value, 10) : 0;
};
const media = breakpoints.flatMap((b) => [
  { name: b.name, query: `(max-width: ${bpPx(b.name)}px)` },
  { name: `above-${b.name}`, query: `(min-width: ${bpPx(b.name) + 1}px)` },
]);
media.push({ name: "reduced-motion", query: "(prefers-reduced-motion: reduce)" });

for (const [group, entries] of Object.entries(source.primitive)) {
  for (const name of Object.keys(entries)) {
    if (!used.has(`${group}.${name}`)) errors.push(`primitive.${group}.${name} is an orphan: no role uses it`);
  }
}

// ---- render ------------------------------------------------------------------------------

const header = (open, line, close) =>
  [
    open,
    `${line}GENERATED FILE. Do not edit by hand.`,
    `${line}Source: packages/tokens/tokens.source.json`,
    `${line}Regenerate: ${COMMAND}`,
    `${line}Drift gate: ${COMMAND} --check`,
    close,
  ].filter((l) => l !== null);

function renderCss() {
  const out = header("/*", " * ", " */");
  out.push("");
  out.push("/* Breakpoints cannot be custom properties inside @media: use the literal queries in MEDIA (tokens.ts). */");
  out.push("");
  for (const f of fontFaces) {
    out.push(
      "@font-face {",
      `  font-family: ${fontFamily};`,
      `  font-style: ${ff.style};`,
      `  font-display: ${ff.display};`,
      `  font-weight: ${f.weight};`,
      `  src: url("../${f.file}") format("woff2");`,
      `  unicode-range: ${f.range};`,
      "}",
      "",
    );
  }
  out.push(":root {");
  for (const [group, list] of Object.entries(roles)) {
    out.push(`  /* ${group} */`);
    for (const r of list) out.push(`  ${r.cssVar}: ${r.value};`);
  }
  out.push("  /* rank */");
  for (const r of ranks) out.push(`  --sr-rank-${kebab(r.key)}: ${r.value};`);
  out.push("  /* grade */");
  for (const g of grades) out.push(`  --sr-grade-${kebab(g.key)}: ${g.value};`);
  out.push("}");
  for (const bp of [...new Set(overrides.map((o) => o.breakpoint))]) {
    out.push("", `@media (max-width: ${bpPx(bp)}px) {`, "  :root {");
    for (const o of overrides.filter((x) => x.breakpoint === bp)) out.push(`    ${o.cssVar}: ${o.value};`);
    out.push("  }", "}");
  }
  for (const p of pages) {
    out.push("", `[data-page="${p.id}"] {`);
    for (const v of p.values) out.push(`  --sr-page-${v.role}: ${v.value};`);
    out.push("}");
  }
  return `${out.join("\n")}\n`;
}

const IDENT = /^[A-Za-z_$][A-Za-z0-9_$]*$/;
const tsKey = (k) => (IDENT.test(k) ? k : JSON.stringify(k));
const tsObject = (pairs, indent) => {
  const pad = " ".repeat(indent);
  return `{\n${pairs.map(([k, v]) => `${pad}  ${tsKey(k)}: ${v},`).join("\n")}\n${pad}}`;
};
const str = (v) => JSON.stringify(v);

function renderTs() {
  const out = header("/*", " * ", " */");
  out.push("");
  // One const per group, so a bundle that reads one group (the wall's layout, a canvas palette)
  // does not carry every other role; TOKENS gathers them all.
  const groupConst = (g) => `TOKENS_${g.replace(/-/g, "_").toUpperCase()}`;
  for (const [g, list] of Object.entries(roles)) {
    out.push(`/** The \`${g}\` roles resolved to their values. */`);
    out.push(`export const ${groupConst(g)} = ${tsObject(list.map((r) => [camel(r.name), str(r.value)]), 0)} as const;`);
  }
  out.push("", "/** Every role resolved to its value (canvas and inline use). Primitives stay in the source JSON. */");
  out.push(`export const TOKENS = ${tsObject(Object.keys(roles).map((g) => [g, groupConst(g)]), 0)} as const;`);
  out.push("", "/** CSS custom-property prefix per role group; a role's property is prefix + kebab-case key. */");
  out.push(`export const VAR_PREFIX = ${tsObject(Object.keys(roles).map((g) => [g, str(PREFIX[g])]), 0)} as const;`);
  out.push("", "/** Values that change at a breakpoint, keyed by breakpoint role then CSS custom property. */");
  const bps = [...new Set(overrides.map((o) => o.breakpoint))];
  out.push(
    `export const AT_BREAKPOINT = ${tsObject(
      bps.map((bp) => [bp, tsObject(overrides.filter((o) => o.breakpoint === bp).map((o) => [o.cssVar, str(o.value)]), 2)]),
      0,
    )} as const;`,
  );
  out.push("", `export const PAGE_IDS = [${pages.map((p) => str(p.id)).join(", ")}] as const;`);
  out.push("export type PageId = (typeof PAGE_IDS)[number];");
  out.push("", '/** Per-page palettes; tokens.css scopes them under [data-page="<id>"]. */');
  out.push(
    `export const PAGE_PALETTES = ${tsObject(
      pages.map((p) => [p.id, tsObject(p.values.map((v) => [camel(v.role), str(v.value)]), 2)]),
      0,
    )} as const;`,
  );
  out.push(`export const PAGE_VARS = ${tsObject(pageRoles.map((r) => [camel(r), str(`--sr-page-${r}`)]), 0)} as const;`);
  out.push("", "/** Viewer rank colours (fidelity role, single source for the console and the stage). */");
  out.push(`export const RANK_COLORS = ${tsObject(ranks.map((r) => [r.key, str(r.value)]), 0)} as const;`);
  out.push("", "/** Evidence grade colours (fidelity role, single source for the console and the stage). */");
  out.push(`export const GRADE_COLORS = ${tsObject(grades.map((g) => [g.key, str(g.value)]), 0)} as const;`);
  out.push("", "/** Breakpoints in CSS px, for matchMedia and canvas layout. */");
  out.push(`export const BREAKPOINTS_PX = ${tsObject(breakpoints.map((b) => [camel(b.name), String(bpPx(b.name))]), 0)} as const;`);
  out.push("", "/** The only media queries components may use (breakpoint roles plus reduced motion). */");
  out.push(`export const MEDIA = ${tsObject(media.map((m) => [camel(m.name), str(m.query)]), 0)} as const;`);
  return `${out.join("\n")}\n`;
}

// ---- run ---------------------------------------------------------------------------------

const outputs = [
  [OUT_CSS, renderCss()],
  [OUT_TS, renderTs()],
];

if (errors.length > 0) {
  for (const e of errors) console.error(`tokens: ${e}`);
  console.error(`tokens: ${errors.length} source error(s); nothing written.`);
  process.exit(1);
}
if (roleCount === 0) {
  console.error("tokens: the source holds no roles; the gate walked nothing.");
  process.exit(2);
}

const summary = `${roleCount} roles, ${used.size} primitives, ${pages.length} page palettes, ${fontFaces.length} font faces`;
if (process.argv.includes("--check")) {
  let drift = 0;
  for (const [file, want] of outputs) {
    const path = join(PKG, file);
    const have = existsSync(path) ? readFileSync(path, "utf8").replace(/\r\n/g, "\n") : null;
    if (have === want) continue;
    drift += 1;
    if (have === null) {
      console.error(`tokens: DRIFT ${file} is missing`);
      continue;
    }
    const a = have.split("\n");
    const b = want.split("\n");
    const line = a.findIndex((l, i) => l !== b[i]);
    const at = line === -1 ? Math.min(a.length, b.length) : line;
    console.error(`tokens: DRIFT ${file} line ${at + 1}`);
    console.error(`  file:   ${a[at] ?? "<end of file>"}`);
    console.error(`  source: ${b[at] ?? "<end of file>"}`);
  }
  if (drift > 0) {
    console.error(`tokens: ${drift} of ${outputs.length} generated files drifted. Run: ${COMMAND}`);
    process.exit(1);
  }
  console.log(`tokens: check OK, ${outputs.length} of ${outputs.length} files match the source (${summary}).`);
} else {
  for (const [file, text] of outputs) writeFileSync(join(PKG, file), text);
  console.log(`tokens: wrote ${outputs.map(([f]) => f).join(", ")} (${summary}).`);
}
