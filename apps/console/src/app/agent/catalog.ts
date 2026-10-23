// The tool catalog (docs/guide/athena.md), a dynamic chunk: boot.ts loads it only inside Athena
// with "Let Athena drive" on. Every tool is registered under one abort signal; aborting it
// unregisters them all (the polyfill listens to the signal; unregisterTool covers a registry
// that does not).
import type { ModelContextLike } from "../../kit/athenaDrive";
import { bind, type ToolSpec } from "./tool";
import { CHARACTER_TOOLS } from "./toolsCharacters";
import { ORIENT_TOOLS } from "./toolsOrient";
import { RUN_TOOLS } from "./toolsRuns";
import { WRITE_TOOLS } from "./toolsWrites";
import type { AgentDeps, AgentTool } from "./types";

export const SPECS: readonly ToolSpec[] = [...ORIENT_TOOLS, ...RUN_TOOLS, ...CHARACTER_TOOLS, ...WRITE_TOOLS];

export function buildTools(deps: AgentDeps): AgentTool[] {
  return SPECS.map((s) => bind(s, deps));
}

export function registerTools(registry: ModelContextLike, deps: AgentDeps, signal: AbortSignal): void {
  if (signal.aborted) return;
  const tools = buildTools(deps);
  for (const tool of tools) registry.registerTool(tool, { signal });
  signal.addEventListener(
    "abort",
    () => {
      for (const tool of tools) registry.unregisterTool?.(tool.name);
    },
    { once: true },
  );
}
