// ui/Drawer: it opens over the shell (portalled to the shell root, above the chrome), has an exit
// (the content holds through the wipe, then unmounts), closes on Esc without the keymap going up
// a level, returns focus to its opener, and keeps the reduced-motion rule.
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { useState } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { DRAWER_EXIT_MS, Drawer } from "./Drawer";

afterEach(() => {
  cleanup();
  vi.useRealTimers();
});

function Host({ onKey }: { onKey?: (e: KeyboardEvent) => void }) {
  const [open, setOpen] = useState(false);
  return (
    <div data-role="shell" onKeyDown={(e) => onKey?.(e.nativeEvent)}>
      <main data-role="spread">
        <button type="button" onClick={() => setOpen(true)}>
          Open the moment
        </button>
      </main>
      <Drawer open={open} onClose={() => setOpen(false)} title="The moment" kicker="00:42 · line 12" page="review" data-role="test-drawer">
        <p>Let the deficit be recorded.</p>
        <textarea aria-label="Note" />
      </Drawer>
    </div>
  );
}

const drawer = () => document.querySelector<HTMLElement>("[data-role='test-drawer']");
const openIt = () => act(async () => fireEvent.click(screen.getByRole("button", { name: "Open the moment" })));

describe("ui/Drawer", () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });

  it("stays mounted but empty and hidden while closed", () => {
    render(<Host />);
    expect(drawer()?.getAttribute("data-open")).toBe("false");
    expect(drawer()?.getAttribute("aria-hidden")).toBe("true");
    expect(drawer()?.childElementCount).toBe(0);
  });

  it("opens beside the spread, not inside it, with its head, the page palette and focus", async () => {
    render(<Host />);
    await openIt();
    const d = drawer();
    expect(d?.getAttribute("data-open")).toBe("true");
    expect(d?.getAttribute("aria-hidden")).toBe("false");
    // Portalled to the shell root: the spread's clip-path stacking context cannot hold it under the chrome.
    expect(d?.parentElement?.getAttribute("data-role")).toBe("shell");
    expect(d?.closest("[data-role='spread']")).toBeNull();
    expect(d?.getAttribute("data-page")).toBe("review");
    expect(screen.getByRole("dialog", { name: "The moment" })).toBe(d);
    expect(screen.getByText("00:42 · line 12")).toBeTruthy();
    expect(document.activeElement).toBe(d);
  });

  it("has an exit: closing keeps the content through the wipe, then unmounts it", async () => {
    render(<Host />);
    await openIt();
    await act(async () => fireEvent.click(screen.getByRole("button", { name: "Close" })));
    expect(drawer()?.getAttribute("data-open")).toBe("false");
    expect(screen.getByText("Let the deficit be recorded.")).toBeTruthy();
    await act(async () => vi.advanceTimersByTime(DRAWER_EXIT_MS - 1));
    expect(screen.queryByText("Let the deficit be recorded.")).not.toBeNull();
    await act(async () => vi.advanceTimersByTime(1));
    expect(screen.queryByText("Let the deficit be recorded.")).toBeNull();
  });

  it("closes on Esc, handled so the keymap does not also go up, and returns focus to the opener", async () => {
    const seen: boolean[] = [];
    render(<Host onKey={(e) => seen.push(e.defaultPrevented)} />);
    const opener = screen.getByRole("button", { name: "Open the moment" });
    opener.focus();
    await openIt();
    await act(async () => fireEvent.keyDown(drawer() as HTMLElement, { key: "Escape" }));
    expect(drawer()?.getAttribute("data-open")).toBe("false");
    expect(seen).toEqual([true]);
    expect(document.activeElement).toBe(opener);
  });

  it("leaves a field on the first Esc and closes on the second", async () => {
    render(<Host />);
    await openIt();
    const note = screen.getByRole("textbox", { name: "Note" });
    note.focus();
    await act(async () => fireEvent.keyDown(note, { key: "Escape" }));
    expect(drawer()?.getAttribute("data-open")).toBe("true");
    expect(document.activeElement).toBe(drawer());
    await act(async () => fireEvent.keyDown(drawer() as HTMLElement, { key: "Escape" }));
    expect(drawer()?.getAttribute("data-open")).toBe("false");
  });

  it("wipes in and out with the shell's ease, lags visibility on the exit, and respects reduced motion", () => {
    const css = readFileSync(join(process.cwd(), "src/ui/Drawer.module.css"), "utf8");
    expect(css).toContain("transition: clip-path .6s var(--sr-ease-move), visibility 0s .6s;");
    expect(css).toContain("z-index: calc(var(--sr-z-chrome) + 1);");
    expect(css).toContain("box-shadow: -.4rem .4rem 0 var(--sr-page-accent, var(--sr-color-signal));");
    expect(css).not.toMatch(/blur\(/);
    expect(css).toMatch(/prefers-reduced-motion: reduce\) \{\s*\.drawer,\s*\.drawer \* \{ animation-duration: \.01s !important;/);
  });
});
