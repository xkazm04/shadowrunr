import { act, cleanup, fireEvent, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { mountPage, ok, param, settle } from "../../kit/testing";
import { Layer2 } from "./Layer2";
import { session } from "./session";

let dispose: (() => void) | undefined;
beforeEach(() => session.reset());
afterEach(() => {
  cleanup();
  dispose?.();
  session.reset();
});

describe("reconcile's default source tournament", () => {
  it("is the newest by updated_ms, not the last in the daemon's name order", async () => {
    const rows = [
      { tournament_id: "a-new", instrument_hash: "h", runs: 4, completed: 4, updated_ms: 200 },
      { tournament_id: "z-old", instrument_hash: "h", runs: 4, completed: 4, updated_ms: 100 },
    ];
    const sent: string[] = [];
    const m = mountPage(
      <Layer2 sub={["report"]} />,
      (method, body) => {
        if (method === "tournament.list") return ok(rows);
        if (method !== "reconcile.run") return ok([]);
        sent.push(body);
        return { hang: true };
      },
      "#/reconcile/report",
    );
    dispose = m.dispose;
    await settle();
    const select = document.querySelector("[data-role='reconcile-empty'] select") as HTMLSelectElement | null;
    expect(select?.value).toBe("a-new");
    await act(async () => fireEvent.click(screen.getByRole("button", { name: "Run reconcile" })));
    await settle();
    expect(sent.map((b) => param(b, "tournament_id"))).toEqual(["a-new"]);
  });
});
