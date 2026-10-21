import { act, cleanup, fireEvent, screen } from "@testing-library/react";
import { useState } from "react";
import { afterEach, describe, expect, it } from "vitest";
import { mountPage, ok, settle } from "../../kit/testing";
import { Layer2 } from "./Layer2";
import { session } from "./session";

afterEach(cleanup);

describe("a reconcile run outlives the tab that started it", () => {
  it("NoReport keeps 'Reconciling…' across reconcile/report -> chat -> report and sends no second run", async () => {
    session.reset();
    // The shell keys layer 2 on the tab (rise.ts), so each switch remounts the page.
    function Tabs() {
      const [tab, setTab] = useState("report");
      return (
        <>
          <button type="button" onClick={() => setTab((t) => (t === "report" ? "chat" : "report"))}>
            switch tab
          </button>
          <Layer2 key={tab} sub={[tab]} />
        </>
      );
    }
    const go = () => fireEvent.click(screen.getByRole("button", { name: "switch tab" }));
    const m = mountPage(<Tabs />, (method) => {
      if (method === "reconcile.run") return { hang: true };
      if (method === "tournament.list") return ok([{ tournament_id: "pilot-d1ef98d3a6", instrument_hash: "abc", runs: 4, completed: 4 }]);
      return ok([]);
    }, "#/reconcile/report");
    try {
      await settle();
      await act(async () => fireEvent.click(screen.getByRole("button", { name: "Run reconcile" })));
      await settle();
      expect(screen.getByRole("button", { name: "Reconciling…" })).toBeTruthy();
      await act(async () => go());
      await settle();
      expect(screen.queryByRole("button", { name: "Reconciling…" })).toBeNull();
      await act(async () => go());
      await settle();
      const button = screen.getByRole("button", { name: "Reconciling…" }) as HTMLButtonElement;
      expect(button.disabled).toBe(true);
      expect(screen.getByRole("status").textContent).toContain("pilot-d1ef98d3a6");
      await act(async () => fireEvent.click(button));
      await act(async () => fireEvent.submit(button.closest("form") as HTMLFormElement));
      await settle();
      expect(m.calls.filter((c) => c === "reconcile.run")).toHaveLength(1);
    } finally {
      m.dispose();
      session.reset();
    }
  });
});
