// The debug view's overflow audit (`?debug=1`).

import { useEffect } from "react";

declare global {
  interface Window {
    /** The debug view's scene probe (`?debug=1`), written by App for browser automation. */
    __stage?: unknown;
    /** The overflow audit's last findings (`?debug=1`). */
    __stageOverflow?: string[];
  }
}

/** Outline anything inside the canvas that overflows its box (`?debug=1`). Re-checks every
 * second, so entry animations that have finished are measured at rest. */
export function useOverflowAudit(enabled: boolean, root: React.RefObject<HTMLDivElement | null>) {
  useEffect(() => {
    if (!enabled) return;
    const audit = () => {
      const canvas = root.current?.querySelector(".canvas");
      if (!canvas) return;
      const found: string[] = [];
      for (const el of canvas.querySelectorAll<HTMLElement>(".region:not(.hidden) *")) {
        el.classList.remove("overflowing");
        if (el.closest("[data-clip]") || el instanceof SVGElement || el.classList.contains("w")) continue;
        const style = getComputedStyle(el);
        if (style.display === "inline") continue;
        // Intentional truncation (line clamp, ellipsis) is not overflow.
        if (style.webkitLineClamp !== "none" && style.webkitLineClamp !== "") continue;
        if (style.textOverflow === "ellipsis") continue;
        if (el.scrollWidth > el.clientWidth + 1 || el.scrollHeight > el.clientHeight + 1) {
          el.classList.add("overflowing");
          found.push(`${el.className || el.tagName} (${el.scrollWidth}x${el.scrollHeight} > ${el.clientWidth}x${el.clientHeight})`);
        }
      }
      window.__stageOverflow = found;
      document.body.dataset.overflow = found.length ? found.join(" | ") : "none";
    };
    audit();
    const id = setInterval(audit, 1000);
    return () => clearInterval(id);
  }, [enabled, root]);
}
