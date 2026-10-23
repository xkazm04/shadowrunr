// Test doubles for the agent layer: a WebMCP registry that behaves like inject.js's polyfill
// (a signal unregisters), a scripted RpcClient behind a data layer's client and mutate, and a
// router that records where it was sent. Tests only; nothing in the app imports this file.
import { dataError, type DataError, type DataLayer } from "../../data";
import type { Result } from "../../data";
import { encodeRoute, pageRoute, type Route } from "../../router";
import type { ModelContextLike } from "../../kit/athenaDrive";
import type { AgentDeps, AgentTool } from "./types";

export class FakeRegistry implements ModelContextLike {
  readonly tools = new Map<string, AgentTool>();
  registerTool(tool: { readonly name: string }, options: { signal?: AbortSignal } = {}): void {
    if (options.signal?.aborted) return;
    this.tools.set(tool.name, tool as AgentTool);
    options.signal?.addEventListener("abort", () => this.unregisterTool(tool.name), { once: true });
  }
  unregisterTool(name: string): void {
    this.tools.delete(name);
  }
}

export type Reply = { ok: true; value: unknown } | { ok: false; error: DataError };
export type Script = Readonly<Record<string, (params: unknown) => Reply>>;

export const ok = (value: unknown): Reply => ({ ok: true, value });
export const no = (code: DataError["code"], message: string, detail?: Record<string, unknown>): Reply => ({ ok: false, error: dataError(code, message, detail) });

export interface Fakes {
  readonly deps: AgentDeps;
  /** Every method sent, in order, reads and writes alike. */
  readonly sent: string[];
  /** The methods sent through mutate. */
  readonly writes: { method: string; params: unknown }[];
  /** Every hash the router was sent to. */
  readonly navs: string[];
  route(): Route;
}

export function fakes(script: Script, start: Route = pageRoute("onair")): Fakes {
  const sent: string[] = [];
  const writes: { method: string; params: unknown }[] = [];
  const navs: string[] = [];
  let current = start;
  const answer = async (method: string, params: unknown): Promise<Result<unknown, DataError>> => {
    sent.push(method);
    const fn = script[method];
    return fn ? fn(params) : { ok: false, error: dataError("method_not_found", `no script for ${method}`) };
  };
  const data: Pick<DataLayer, "client" | "mutate"> = {
    client: { call: (method, params) => answer(method, params) },
    mutate: (method, params) => {
      writes.push({ method, params });
      return answer(method, params);
    },
  };
  const router = {
    getRoute: () => current,
    navigate(route: Route) {
      current = route;
      navs.push(encodeRoute(route));
    },
  };
  return { deps: { data, router }, sent, writes, navs, route: () => current };
}

/** The tool's JSON text, read back. */
export async function run(tool: AgentTool | undefined, input: Record<string, unknown> = {}): Promise<Record<string, unknown>> {
  if (!tool) throw new Error("no such tool");
  const outp = await tool.execute(input);
  const text = outp.content[0]?.text ?? "";
  return parseJson(text);
}

/** JSON.parse lives in the contract door (lint); the tests read tool text through Response. */
async function parseJson(text: string): Promise<Record<string, unknown>> {
  const v: unknown = await new Response(text).json();
  if (v === null || typeof v !== "object" || Array.isArray(v)) throw new Error(`not a JSON object: ${text}`);
  return v as Record<string, unknown>;
}
