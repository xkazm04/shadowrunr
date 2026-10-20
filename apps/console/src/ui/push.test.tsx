// The push-in, moved out of the shell so pages reach it: the transform from a clicked rect, and
// PushIn replaying it on an in-page list-to-detail move (never on first render, never under
// reduced motion).
import { cleanup, render } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { pushFrom } from "./push";
import { PushIn } from "./PushIn";

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

const row = () => {
  const el = document.createElement("div");
  el.getBoundingClientRect = () => ({ left: 0, top: 400, width: 800, height: 40, right: 800, bottom: 440, x: 0, y: 400, toJSON: () => ({}) });
  document.body.append(el);
  return el;
};

describe("pushFrom", () => {
  it("translates centre to centre and scales to the origin, never below .15", () => {
    expect(pushFrom({ left: 0, top: 400, width: 800, height: 40 }, { left: 0, top: 0, width: 800, height: 400 })).toBe("translate(0.0px, 220.0px) scale(1.000, 0.150)");
    expect(pushFrom({ left: 100, top: 100, width: 200, height: 200 }, { left: 0, top: 0, width: 400, height: 400 })).toBe("translate(0.0px, 0.0px) scale(0.500, 0.500)");
  });
});

describe("PushIn", () => {
  it("pushes in from the picked row when the trigger changes, not on the first render", () => {
    const from = row();
    const view = render(<PushIn trigger="a" from={() => from}>detail a</PushIn>);
    const box = view.container.firstElementChild as HTMLElement;
    expect(box.dataset["push"]).toBeUndefined();
    view.rerender(<PushIn trigger="b" from={() => from}>detail b</PushIn>);
    expect(box.dataset["push"]).toBe("true");
    expect(box.style.getPropertyValue("--push-from")).toMatch(/^translate\(/);
  });

  it("only appears under reduced motion, or with no row to come from", () => {
    vi.stubGlobal("matchMedia", (q: string) => ({ matches: q.includes("reduce") }));
    const from = row();
    const view = render(<PushIn trigger="a" from={() => from}>a</PushIn>);
    view.rerender(<PushIn trigger="b" from={() => from}>b</PushIn>);
    expect((view.container.firstElementChild as HTMLElement).dataset["push"]).toBeUndefined();
    vi.unstubAllGlobals();
    view.rerender(<PushIn trigger="c" from={() => null}>c</PushIn>);
    expect((view.container.firstElementChild as HTMLElement).dataset["push"]).toBeUndefined();
  });
});
