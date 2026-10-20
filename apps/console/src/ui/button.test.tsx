// ui/Button: the variants, sizes and native props, its look, and the stylesheets it replaced.
// The pages that render it are tested beside them (reconcile/buttons, onair/clips.buttons).
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { Button, buttonClass } from "./Button";
import styles from "./Button.module.css";

afterEach(cleanup);

const classes = (el: Element) => [...el.classList];

describe("ui/Button", () => {
  it("renders a native button of each variant, `button` unless it says otherwise", () => {
    render(
      <>
        <Button>Paper</Button>
        <Button variant="primary" type="submit">
          Accent
        </Button>
        <Button variant="ink" size="compact">
          Ink
        </Button>
      </>,
    );
    const paper = screen.getByRole("button", { name: "Paper" });
    const accent = screen.getByRole("button", { name: "Accent" });
    const ink = screen.getByRole("button", { name: "Ink" });
    expect([paper, accent, ink].map((b) => b.getAttribute("type"))).toEqual(["button", "submit", "button"]);
    expect([paper, accent, ink].map((b) => b.getAttribute("data-variant"))).toEqual(["secondary", "primary", "ink"]);
    expect(classes(paper)).toEqual([styles.button]);
    expect(classes(accent)).toEqual([styles.button, styles.primary]);
    expect(classes(ink)).toEqual([styles.button, styles.ink, styles.compact]);
  });

  it("passes native props through and stays inert while disabled", () => {
    const onClick = vi.fn();
    render(
      <Button disabled onClick={onClick} aria-label="Seal" className="extra" data-role="seal">
        Seal this plan
      </Button>,
    );
    const b = screen.getByRole("button", { name: "Seal" });
    fireEvent.click(b);
    expect(onClick).not.toHaveBeenCalled();
    expect((b as HTMLButtonElement).disabled).toBe(true);
    expect(classes(b)).toContain("extra");
    expect(b.getAttribute("data-role")).toBe("seal");
  });

  it("lends its look to a link through buttonClass", () => {
    expect(buttonClass()).toBe(styles.button);
    expect(buttonClass("ink", "compact", "x")).toBe(`${styles.button} ${styles.ink} ${styles.compact} x`);
  });

  it("lifts on hover, collapses its offset when pressed, rings focus in currentcolor and holds still for reduced motion", () => {
    const css = readFileSync(join(process.cwd(), "src/ui/Button.module.css"), "utf8");
    expect(css).toMatch(/\.button:hover:not\(:disabled\) \{ transform: translate\(-\.1rem, -\.1rem\); box-shadow: \.3rem \.3rem 0 var\(--btn-offset\); \}/);
    expect(css).toMatch(/\.button:active:not\(:disabled\) \{[^}]*box-shadow: 0 0 0/);
    expect(css).toMatch(/\.button:focus-visible \{ outline: var\(--sr-stroke-heavy\) solid currentcolor;/);
    expect(css).toMatch(/prefers-reduced-motion: reduce\) \{\s*\.button \{ transition: none; \}/);
  });
});

describe("the pages on ui/Button", () => {
  it("left no duplicated button rules behind in the migrated stylesheets", () => {
    for (const f of ["src/pages/reconcile/Layer2.module.css", "src/pages/onair/Clips.module.css", "src/pages/onair/Layer2.module.css"]) {
      const css = readFileSync(join(process.cwd(), f), "utf8");
      expect(css, f).not.toMatch(/^\.(primary|secondary)\b/m);
    }
  });
});
