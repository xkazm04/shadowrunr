// The agent layer's shapes. Types only: boot.ts (first load) imports this file, so nothing
// here may produce a byte of runtime code.
import type { ModelContextLike } from "../../kit/athenaDrive";
import type { DataLayer } from "../../data";
import type { RouterStore } from "../../router";

/** What the tools reach: the console's own data layer and hash router, nothing else. */
export interface AgentDeps {
  readonly data: Pick<DataLayer, "client" | "mutate">;
  readonly router: Pick<RouterStore, "getRoute" | "navigate">;
}

/** The three classes of the design brief, each one fixed pair of hints plus the athena block. */
export type ToolClass = "READ" | "AUTO" | "GATED";

export interface JsonSchema {
  readonly [key: string]: unknown;
}

/** What a tool returns to the bridge: inject.js flattens `content` to its text parts. */
export interface ToolOutput {
  readonly content: readonly { readonly type: "text"; readonly text: string }[];
}

/** One tool as `document.modelContext.registerTool` takes it. */
export interface AgentTool {
  readonly name: string;
  readonly title: string;
  readonly description: string;
  readonly inputSchema: JsonSchema;
  readonly annotations: { readonly readOnlyHint: boolean; readonly consequentialHint: boolean };
  readonly athena: { readonly reversible: boolean; readonly side_effects: "none" | "internal" };
  execute(input: Record<string, unknown>, options?: { signal?: AbortSignal }): Promise<ToolOutput>;
}

/** The dynamic chunk's surface. */
export interface Catalog {
  registerTools(registry: ModelContextLike, deps: AgentDeps, signal: AbortSignal): void;
}
