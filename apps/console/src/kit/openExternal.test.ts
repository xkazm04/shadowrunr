// The opener (Tauri's plugin inside the desktop shell, a new tab in a plain browser) and the
// stage replay URL it is handed.
import { afterEach, describe, expect, it, vi } from "vitest";
import { DEFAULT_STAGE_URL, readDataConfig, stageReplayUrl } from "../data/config";
import { emitKeyCommand } from "./keyCommands";
import { inTauri, openExternal } from "./openExternal";

const openUrl = vi.hoisted(() => vi.fn((_url: string) => Promise.resolve()));
vi.mock("@tauri-apps/plugin-opener", () => ({ openUrl }));

const TAURI_KEY = "__TAURI_INTERNALS__";

afterEach(() => {
  Reflect.deleteProperty(window, TAURI_KEY);
  vi.restoreAllMocks();
  openUrl.mockClear();
});

describe("openExternal", () => {
  it("opens a new tab without an opener reference in a plain browser", async () => {
    const open = vi.spyOn(window, "open").mockImplementation(() => null);
    expect(inTauri()).toBe(false);
    await openExternal("http://localhost:5178/?run=r1");
    expect(open).toHaveBeenCalledWith("http://localhost:5178/?run=r1", "_blank", "noopener");
    expect(openUrl).not.toHaveBeenCalled();
  });

  it("hands the URL to Tauri's opener plugin inside the desktop shell", async () => {
    Reflect.set(window, TAURI_KEY, {});
    const open = vi.spyOn(window, "open").mockImplementation(() => null);
    expect(inTauri()).toBe(true);
    await openExternal("http://localhost:5178/?run=r1");
    expect(openUrl).toHaveBeenCalledWith("http://localhost:5178/?run=r1");
    expect(open).not.toHaveBeenCalled();
  });

  it("passes the plugin's refusal to the caller", async () => {
    Reflect.set(window, TAURI_KEY, {});
    openUrl.mockImplementationOnce(() => Promise.reject(new Error("not allowed")));
    await expect(openExternal("http://x/")).rejects.toThrow("not allowed");
  });
});

describe("stage URL", () => {
  it("defaults to the stage dev server and takes ?stage= or VITE_STAGE_URL", () => {
    expect(readDataConfig("", {}).stageUrl).toBe(DEFAULT_STAGE_URL);
    expect(readDataConfig("", { VITE_STAGE_URL: "http://127.0.0.1:9000/" }).stageUrl).toBe("http://127.0.0.1:9000/");
    expect(readDataConfig("?stage=http://h:1/s/", { VITE_STAGE_URL: "http://127.0.0.1:9000/" }).stageUrl).toBe("http://h:1/s/");
    expect(readDataConfig("?daemon=http://d:1", {}).daemonUrl).toBe("http://d:1");
  });

  it("asks the stage to replay a run, keeping the base's own query", () => {
    expect(stageReplayUrl("t-0/run 1", "http://localhost:5178/")).toBe("http://localhost:5178/?run=t-0%2Frun+1");
    expect(stageReplayUrl("r1", "http://h:1/stage/?speed=2")).toBe("http://h:1/stage/?speed=2&run=r1");
    expect(stageReplayUrl("r1", "not a url")).toBe("http://localhost:5178/?run=r1");
    expect(stageReplayUrl("r1")).toBe("http://localhost:5178/?run=r1");
  });
});

describe("key commands", () => {
  it("reports an unowned command as unhandled", () => {
    expect(emitKeyCommand("rerun")).toBe(false);
  });
});
