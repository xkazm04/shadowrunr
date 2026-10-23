// The pieces every tool is made of: the class flags, the JSON result, the bounded list, the
// error, the schema builders and the call context (the data layer and the router).
import type { RpcMethod, RpcParams, RpcResult } from "@shadowrunr/contract";
import type { DataError } from "../../data";
import { encodeRoute, type Route } from "../../router";
import type { AgentDeps, AgentTool, JsonSchema, ToolClass, ToolOutput } from "./types";

/** The class table of the design brief: both hints always explicit, plus the athena block. */
export const FLAGS: { readonly [C in ToolClass]: Pick<AgentTool, "annotations" | "athena"> } = {
  READ: { annotations: { readOnlyHint: true, consequentialHint: false }, athena: { reversible: true, side_effects: "none" } },
  AUTO: { annotations: { readOnlyHint: false, consequentialHint: false }, athena: { reversible: true, side_effects: "internal" } },
  GATED: { annotations: { readOnlyHint: false, consequentialHint: true }, athena: { reversible: false, side_effects: "internal" } },
};

/**
 * A failed call. The bridge relays a throw as `{ ok: false, error: <message> }`, so the message
 * leads with the code: `<code>: <message>`, the daemon's own code when the daemon refused.
 */
export class ToolError extends Error {
  readonly code: string;
  constructor(code: string, message: string) {
    super(`${code}: ${message}`);
    this.code = code;
  }
}

export function fail(code: string, message: string): never {
  throw new ToolError(code, message);
}

/** The daemon's error, with its validation issues when it sent any (character.doc.set). */
export function failOf(e: DataError): never {
  const issues = e.detail?.["issues"];
  const extra = Array.isArray(issues) && issues.length > 0 ? ` Issues: ${JSON.stringify(issues.slice(0, 5))}` : "";
  return fail(e.code, `${e.message}${extra}`);
}

export function out(body: Readonly<Record<string, unknown>>): ToolOutput {
  return { content: [{ type: "text", text: JSON.stringify(body) }] };
}

export interface Bounded<T> {
  readonly items: readonly T[];
  readonly showing: number;
  readonly total: number;
  readonly footer: string;
}

/** At most `limit` items from `from` (0-based), with the "showing N of M" footer. */
export function bounded<T>(all: readonly T[], limit: number, from = 0, total = all.length): Bounded<T> {
  const items = all.slice(from, from + limit);
  const more = from + items.length < total ? `; more follow (raise limit or page on)` : "";
  return { items, showing: items.length, total, footer: `showing ${items.length} of ${total}${more}` };
}

// ---- the result budget ----

/**
 * The most JSON text one result may carry. Athena cuts every host tool result at 1,600 chars
 * (athena-portable src/athena/lane/turn_frame.py RESULT_CAP), so a result is designed to fit
 * under it with room to spare, and the cut never lands inside the JSON.
 */
export const RESULT_BUDGET = 1500;

export const sizeOf = (body: unknown): number => JSON.stringify(body).length;

type Rec = Record<string, unknown>;
const isRec = (v: unknown): v is Rec => v !== null && typeof v === "object" && !Array.isArray(v);
/** A bounded list: `items` with its footer, the shape every list answer carries. */
const isList = (v: unknown): v is Rec & { items: unknown[] } => isRec(v) && Array.isArray(v["items"]) && typeof v["footer"] === "string";

/**
 * Fits a body to the budget by trimming its bounded lists (the body itself, or one a level
 * down), longest first, and saying so in each trimmed list's footer. A list that pages by cursor
 * loses its cursor, since the cursor would skip the trimmed items. Returns undefined when no
 * trimming fits: the caller refuses rather than let the cut land mid-JSON.
 */
export function fitBody(body: Rec, budget = RESULT_BUDGET): Rec | undefined {
  if (sizeOf(body) <= budget) return body;
  const root: Rec = isList(body) ? { ...body, items: [...body.items] } : { ...body };
  const lists: { list: Rec & { items: unknown[] }; was: number; cursor: boolean }[] = [];
  if (isList(root)) lists.push({ list: root, was: root.items.length, cursor: false });
  for (const [k, v] of Object.entries(root)) {
    if (k === "items" || !isList(v)) continue;
    const copy = { ...v, items: [...v.items] };
    root[k] = copy;
    lists.push({ list: copy, was: copy.items.length, cursor: false });
  }
  if (lists.length === 0) return undefined;
  // No list can keep more items than fit the budget alone: cut there first, then pop.
  for (const l of lists) {
    let used = 0;
    const keep = l.list.items.findIndex((it) => (used += sizeOf(it) + 1) > budget);
    if (keep >= 0) l.list.items.length = keep + 1;
  }
  while (sizeOf(root) > budget) {
    const l = lists.reduce((a, b) => (sizeOf(b.list.items) > sizeOf(a.list.items) ? b : a));
    if (l.list.items.length === 0) return undefined;
    l.list.items.pop();
    const n = l.list.items.length;
    if ("next_cursor" in l.list) {
      delete l.list["next_cursor"];
      l.cursor = true;
    }
    if ("showing" in l.list) l.list["showing"] = n;
    const total = typeof l.list["total"] === "number" ? l.list["total"] : l.was;
    const how = l.cursor ? `ask again with limit ${Math.max(1, n)} to page by cursor` : "ask for fewer or page on";
    l.list["footer"] = `showing ${n} of ${total}; trimmed from ${l.was} to fit the result budget, ${how}`;
  }
  return root;
}

/** The longest prefix of `items` whose JSON array fits `room` chars. */
export function fitting<T>(items: readonly T[], room: number): T[] {
  const out: T[] = [];
  let used = 2;
  for (const it of items) {
    used += sizeOf(it) + (out.length > 0 ? 1 : 0);
    if (used > room) break;
    out.push(it);
  }
  return out;
}

/** `text` clipped so that adding it under `key` keeps `body` within the budget (omitted if no room). */
export function withRoomFor(body: Rec, key: string, text: string | undefined, max: number, budget = RESULT_BUDGET): Rec {
  if (text === undefined) return body;
  const room = budget - sizeOf({ ...body, [key]: "" });
  let n = Math.min(max, text.length);
  while (n > 0 && sizeOf(text.slice(0, n)) + 1 > room) n -= Math.max(1, Math.ceil(n / 10));
  if (n < Math.min(40, text.length)) return body;
  return { ...body, [key]: n >= text.length ? text : `${text.slice(0, n - 1)}…` };
}

/**
 * The bind guard: a body over the budget is trimmed, or else a read is refused with what to ask
 * instead. A write has already happened by then, so it is never reported as failed: it answers
 * that it was done and that its answer was too long to show.
 */
export function guard(name: string, cls: ToolClass, body: Rec): Rec {
  const fitted = fitBody(body);
  if (fitted !== undefined) return fitted;
  const why = `${name}'s answer is ${sizeOf(body)} chars, over the ${RESULT_BUDGET}-char result budget`;
  if (cls !== "READ") return { done: true, footer: `${why}; read the result back with a read tool` };
  return fail("too_large", `${why}; ask for less (a smaller limit, a later offset or a narrower id)`);
}

// ---- schemas ----

export const obj = (properties: Readonly<Record<string, JsonSchema>>, required: readonly string[] = []): JsonSchema => ({
  type: "object",
  properties,
  required,
  additionalProperties: false,
});

export const str = (description: string, extra: JsonSchema = {}): JsonSchema => ({ type: "string", description, ...extra });
export const int = (description: string, minimum: number, maximum?: number): JsonSchema =>
  maximum === undefined ? { type: "integer", description, minimum } : { type: "integer", description, minimum, maximum };
export const SHOW: JsonSchema = { type: "boolean", description: "Move the console to the fetched view (default true)." };
export const limit = (dflt: number, max: number): JsonSchema => int(`How many (default ${dflt}, at most ${max}).`, 1, max);

// ---- arguments (the bridge checks the schema; these guard a direct call) ----

export function argStr(input: Record<string, unknown>, key: string): string {
  const v = input[key];
  if (typeof v !== "string" || v.length === 0) fail("invalid_params", `"${key}" must be a non-empty string`);
  return v;
}

export function optStr(input: Record<string, unknown>, key: string): string | undefined {
  const v = input[key];
  return typeof v === "string" && v.length > 0 ? v : undefined;
}

export function argInt(input: Record<string, unknown>, key: string, dflt: number, min: number, max: number): number {
  const v = input[key];
  if (v === undefined) return dflt;
  if (typeof v !== "number" || !Number.isInteger(v) || v < min || v > max) fail("invalid_params", `"${key}" must be an integer from ${min} to ${max}`);
  return v;
}

// ---- the call context ----

export interface Ctx {
  readonly deps: AgentDeps;
  readonly signal: AbortSignal | undefined;
  /** A read through the data layer (the door validates the reply); a daemon error throws. */
  read<M extends RpcMethod>(method: M, params: RpcParams<M>): Promise<RpcResult<M>>;
  /** A write through the data layer's mutate (it invalidates what the write changed). */
  write<M extends RpcMethod>(method: M, params: RpcParams<M>): Promise<RpcResult<M>>;
  /** Navigates unless `show` is false; returns the hash shown, or undefined. */
  show(input: Record<string, unknown>, route: Route): string | undefined;
}

export function ctxOf(deps: AgentDeps, signal: AbortSignal | undefined): Ctx {
  const opts = signal === undefined ? undefined : { signal };
  return {
    deps,
    signal,
    async read(method, params) {
      const r = await deps.data.client.call(method, params, opts);
      if (!r.ok) failOf(r.error);
      return r.value as RpcResult<typeof method>;
    },
    async write(method, params) {
      const r = await deps.data.mutate(method, params, opts);
      if (!r.ok) failOf(r.error);
      return r.value as RpcResult<typeof method>;
    },
    show(input, route) {
      if (input["show"] === false) return undefined;
      deps.router.navigate(route);
      return encodeRoute(route);
    },
  };
}

export interface ToolSpec {
  readonly name: string;
  readonly title: string;
  readonly description: string;
  readonly cls: ToolClass;
  readonly input: JsonSchema;
  run(input: Record<string, unknown>, ctx: Ctx): Promise<Readonly<Record<string, unknown>>>;
}

/** A spec bound to the console: the flags of its class, and JSON text out. */
export function bind(spec: ToolSpec, deps: AgentDeps): AgentTool {
  return {
    name: spec.name,
    title: spec.title,
    description: spec.description,
    inputSchema: spec.input,
    ...FLAGS[spec.cls],
    async execute(input, options) {
      const body = await spec.run(input ?? {}, ctxOf(deps, options?.signal));
      return out(guard(spec.name, spec.cls, body));
    },
  };
}
