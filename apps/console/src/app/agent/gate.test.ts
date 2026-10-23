// The registration gate: tools exist on the registry only while BOTH a WebMCP registry is
// present and the operator's opt-in is "1"; switching off aborts and unregisters them all.
import { describe, expect, it } from "vitest";
import { createDrive, DRIVE_KEY, type ModelContextLike } from "../../kit/athenaDrive";
import pkg from "../../../package.json";
import html from "../../../index.html?raw";
import { startAgent } from "./boot";
import { SPECS } from "./catalog";
import { fakes, FakeRegistry } from "./testkit";
import type { Catalog } from "./types";

function memoryStorage(initial: Record<string, string> = {}) {
  const m = new Map(Object.entries(initial));
  return { getItem: (k: string) => m.get(k) ?? null, setItem: (k: string, v: string) => void m.set(k, v), map: m };
}

/** The real catalog, with the loads counted and awaited. */
function loader() {
  const pending: Promise<Catalog>[] = [];
  const load = () => {
    const p = import("./catalog");
    pending.push(p);
    return p;
  };
  return { load, pending, settle: async () => void (await Promise.all(pending)) };
}

function setup(opts: { registry?: ModelContextLike; stored?: string }) {
  const storage = memoryStorage(opts.stored === undefined ? {} : { [DRIVE_KEY]: opts.stored });
  const doc = opts.registry === undefined ? {} : { modelContext: opts.registry };
  const drive = createDrive(() => doc, () => storage);
  const l = loader();
  const stop = startAgent(fakes({}).deps, drive, l.load);
  return { drive, storage, ...l, stop };
}

describe("the registration gate", () => {
  it("registers every tool when the registry exists and the opt-in is on", async () => {
    const registry = new FakeRegistry();
    const s = setup({ registry, stored: "1" });
    await s.settle();
    expect([...registry.tools.keys()].sort()).toEqual(SPECS.map((t) => t.name).sort());
    s.stop();
  });

  it("registers nothing, and loads no catalog, outside Athena even with the opt-in on", async () => {
    const s = setup({ stored: "1" });
    await s.settle();
    expect(s.pending).toHaveLength(0);
    s.stop();
  });

  it("registers nothing inside Athena while the opt-in is off or unset", async () => {
    for (const stored of [undefined, "0", "true"]) {
      const registry = new FakeRegistry();
      const s = setup({ registry, ...(stored === undefined ? {} : { stored }) });
      await s.settle();
      expect(registry.tools.size).toBe(0);
      expect(s.pending).toHaveLength(0);
      s.stop();
    }
  });

  it("toggles on, off and on again: off unregisters every tool, on registers them again", async () => {
    const registry = new FakeRegistry();
    const s = setup({ registry });
    s.drive.set(true);
    await s.settle();
    expect(registry.tools.size).toBe(SPECS.length);
    expect(s.storage.map.get(DRIVE_KEY)).toBe("1");
    s.drive.set(false);
    expect(registry.tools.size).toBe(0);
    expect(s.storage.map.get(DRIVE_KEY)).toBe("0");
    s.drive.set(true);
    await s.settle();
    expect(registry.tools.size).toBe(SPECS.length);
    s.stop();
    expect(registry.tools.size).toBe(0);
  });

  it("unregisters by name too, for a registry that ignores the signal", async () => {
    const names = new Set<string>();
    const deaf: ModelContextLike = { registerTool: (t) => void names.add(t.name), unregisterTool: (n) => void names.delete(n) };
    const s = setup({ registry: deaf, stored: "1" });
    await s.settle();
    expect(names.size).toBe(SPECS.length);
    s.drive.set(false);
    expect(names.size).toBe(0);
    s.stop();
  });

  it("switching off before the catalog arrives registers nothing", async () => {
    const registry = new FakeRegistry();
    const s = setup({ registry, stored: "1" });
    s.drive.set(false);
    await s.settle();
    expect(registry.tools.size).toBe(0);
    s.stop();
  });

  it("reads blocked storage as off and never throws", () => {
    const drive = createDrive(
      () => ({ modelContext: new FakeRegistry() }),
      () => ({
        getItem: () => {
          throw new Error("SecurityError");
        },
        setItem: () => {
          throw new Error("SecurityError");
        },
      }),
    );
    expect(drive.get()).toBe(false);
    expect(() => drive.set(true)).not.toThrow();
    expect(drive.get()).toBe(true);
  });
});

describe("the page identity", () => {
  it("publishes the athena:app meta tags, the version matching package.json", () => {
    expect(html).toContain('<meta name="athena:app" content="shadowrunr" />');
    expect(html).toContain('<meta name="athena:app-name" content="Shadowrunr OS" />');
    expect(html).toContain(`<meta name="athena:app-version" content="${pkg.version}" />`);
  });
});
