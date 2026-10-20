// Reconcile's buttons are ui/Button: the empty report's run and open, and a link that acts as one.
import { cleanup, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { mountPage, ok, settle } from "../../kit/testing";
import { buttonClass } from "../../ui/Button";
import { Layer2 } from "./Layer2";
import { session } from "./session";

let dispose: (() => void) | undefined;
beforeEach(() => session.reset());
afterEach(() => {
  cleanup();
  dispose?.();
});

const mount = (sub: string[]) => {
  const m = mountPage(<Layer2 sub={sub} />, (method) => (method === "tournament.list" ? ok([{ tournament_id: "pilot-1", instrument_hash: "abc", runs: 4, completed: 4 }]) : ok([])), `#/reconcile/${sub.join("/")}`);
  dispose = m.dispose;
};

describe("reconcile on ui/Button", () => {
  it("runs and opens a report through a primary and a secondary Button", async () => {
    mount(["report"]);
    await settle();
    const run = screen.getByRole("button", { name: "Run reconcile" });
    expect(run.getAttribute("data-variant")).toBe("primary");
    expect(run.className).toBe(buttonClass("primary"));
    expect(screen.getByRole("button", { name: "Open" }).className).toBe(buttonClass("secondary"));
  });

  it("gives the unknown page's way back the secondary look", async () => {
    mount(["nowhere"]);
    await settle();
    expect(screen.getByRole("link", { name: "Open the report" }).className).toBe(buttonClass("secondary"));
  });
});
