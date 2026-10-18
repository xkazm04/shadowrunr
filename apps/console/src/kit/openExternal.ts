// Open a URL outside the console: the system browser through Tauri's opener plugin when the
// console runs in the desktop shell (src-tauri registers tauri-plugin-opener and grants
// `opener:allow-open-url`), a new tab in a plain browser.
import { openUrl } from "@tauri-apps/plugin-opener";

/** Tauri injects its IPC bridge before the page runs; a plain browser has none. */
export function inTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

export async function openExternal(url: string): Promise<void> {
  if (inTauri()) {
    await openUrl(url);
    return;
  }
  window.open(url, "_blank", "noopener");
}
