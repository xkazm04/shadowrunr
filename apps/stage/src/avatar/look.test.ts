// The stage's look box: where Malakor looks, told to a face as an event so no face has to poll it.
import { describe, expect, it, vi } from "vitest";

import { LOOK_CHAT, LOOK_HOST, lookBox } from "./look";

describe("lookBox", () => {
  it("tells a subscriber about a real change only, once, and never after it unsubscribes", () => {
    const box = lookBox();
    const fn = vi.fn();
    const off = box.subscribe(fn);
    box.set(LOOK_CHAT);
    expect(fn).toHaveBeenCalledTimes(1);
    expect(fn).toHaveBeenCalledWith({ x: 1, y: -0.1 });
    box.set(LOOK_CHAT);
    box.set({ x: 1, y: -0.1 }); // an equal look is not news either
    expect(fn).toHaveBeenCalledTimes(1);
    off();
    box.set(LOOK_HOST);
    expect(fn).toHaveBeenCalledTimes(1);
    expect(box.get()).toEqual(LOOK_HOST);
  });
});
