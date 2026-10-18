// Open a URL outside the console: the system browser inside Tauri (plugin-opener), a new tab
// in a plain browser.

export function inTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

export async function openExternal(url: string): Promise<void> {
  if (inTauri()) {
    const { openUrl } = await import("@tauri-apps/plugin-opener");
    await openUrl(url);
    return;
  }
  window.open(url, "_blank", "noopener");
}

/** The stage replays a run with `?run=<id>` (apps/stage, WP8). */
export function stageReplayUrl(stageUrl: string, runId: string): string {
  const u = new URL(stageUrl);
  u.searchParams.set("run", runId);
  return u.toString();
}
