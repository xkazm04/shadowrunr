import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { DAEMON_START_COMMAND, RpcError } from "../api/rpc";
import { ErrorState } from "./primitives";

afterEach(cleanup);

describe("<ErrorState>", () => {
  it("shows the start command and a fixtures link when the daemon is offline", () => {
    render(<ErrorState error={new RpcError("offline", "tournament.list", "Cannot reach")} onRetry={() => {}} />);
    expect(screen.getByText("The daemon is not running")).toBeTruthy();
    expect(screen.getByText(DAEMON_START_COMMAND)).toBeTruthy();
    expect(screen.getByText("Review with fixtures").getAttribute("href")).toContain("mock=1");
  });

  it("shows the method, code and message for a daemon-side error", () => {
    render(<ErrorState error={new RpcError("rpc", "sim.start", "PF-07 is red", { code: "preflight_red" })} />);
    expect(screen.getByText("sim.start failed")).toBeTruthy();
    expect(screen.getByText("preflight_red")).toBeTruthy();
    expect(screen.getByText("PF-07 is red")).toBeTruthy();
    expect(screen.queryByText(DAEMON_START_COMMAND)).toBeNull();
  });
});
