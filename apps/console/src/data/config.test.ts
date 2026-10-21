import { describe, expect, it } from "vitest";
import { readDataConfig } from "./config";
import { dataError, unavailableHint, viewOfError } from "./errors";

describe("readDataConfig daemon precedence", () => {
  const search = "?daemon=http://127.0.0.1:1";
  const env = { VITE_DAEMON_URL: "http://127.0.0.1:2" };

  it("the shell value wins: it is the only origin the desktop window's CSP admits", () => {
    expect(readDataConfig(search, env, "http://127.0.0.1:9000").daemonUrl).toBe("http://127.0.0.1:9000");
  });

  it("without a shell value the query, then the build env, then the default decide, as before", () => {
    expect(readDataConfig(search, env, undefined).daemonUrl).toBe("http://127.0.0.1:1");
    expect(readDataConfig("", env, undefined).daemonUrl).toBe("http://127.0.0.1:2");
    expect(readDataConfig("", {}, undefined).daemonUrl).toBe("http://127.0.0.1:7717");
  });
});

describe("the offline remedy names the daemon in use", () => {
  it("prints the resolved port, not the literal default", () => {
    const v = viewOfError(dataError("unavailable", "offline", { reason: "offline" }), "http://127.0.0.1:9000");
    expect(v.hint).toBe("Start it with `shadowrunr daemon --port 9000`; the console reconnects on its own.");
    expect(unavailableHint("http://127.0.0.1:9000")).not.toContain("7717");
  });

  it("the default daemon keeps today's remedy", () => {
    expect(unavailableHint("http://127.0.0.1:7717")).toBe("Start it with `shadowrunr daemon --port 7717`; the console reconnects on its own.");
  });
});
