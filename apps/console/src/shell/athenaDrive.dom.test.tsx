// The "Let Athena drive" toggle beside the connection chip: absent outside Athena, and inside
// it a pressed/unpressed button whose label and stored opt-in follow the click.
import { act, cleanup, fireEvent } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { drive, DRIVE_KEY } from "../kit/athenaDrive";
import { mountPage, offline, role } from "../kit/testing";
import { Chrome } from "./Chrome";

const chrome = <Chrome onWall={false} narrow={false} onToggleWall={() => undefined} onGoTo={() => undefined} />;

afterEach(() => {
  cleanup();
  act(() => drive.set(false));
  Reflect.deleteProperty(document, "modelContext");
  localStorage.removeItem(DRIVE_KEY);
});

describe("the Athena toggle", () => {
  it("is not rendered without document.modelContext", () => {
    const m = mountPage(chrome, offline);
    expect(role("connection")).not.toBeNull();
    expect(role("athena-drive")).toBeNull();
    m.dispose();
  });

  it("inside Athena reads Athena when off and Athena driving when on, and stores the opt-in", () => {
    Object.defineProperty(document, "modelContext", { value: { registerTool: () => undefined }, configurable: true });
    const m = mountPage(chrome, offline);
    const btn = role("athena-drive");
    expect(btn?.textContent).toBe("Athena");
    expect(btn?.getAttribute("aria-pressed")).toBe("false");
    fireEvent.click(btn as Element);
    expect(role("athena-drive")?.textContent).toBe("Athena driving");
    expect(role("athena-drive")?.getAttribute("aria-pressed")).toBe("true");
    expect(localStorage.getItem(DRIVE_KEY)).toBe("1");
    fireEvent.click(role("athena-drive") as Element);
    expect(role("athena-drive")?.textContent).toBe("Athena");
    expect(localStorage.getItem(DRIVE_KEY)).toBe("0");
    m.dispose();
  });
});
