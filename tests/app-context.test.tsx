import { describe, expect, test } from "bun:test";
import { testRender } from "@opentui/solid";
import { createRoot } from "solid-js";
import { AppCtx, useAppContext, type AppContextValue } from "../src/app-context";
import { makeAppContextValue } from "./helpers";

describe("AppContext", () => {
  test("throws a clear error when no provider is present", () => {
    createRoot((dispose) => {
      expect(() => useAppContext()).toThrow("useAppContext must be used within AppCtx");
      dispose();
    });
  });

  test("returns the provided context value", async () => {
    const value: AppContextValue = makeAppContextValue();
    let resolved: AppContextValue | undefined;

    function Probe() {
      resolved = useAppContext();
      return null;
    }

    const { renderOnce } = await testRender(
      () => (
        <AppCtx value={value}>
          <Probe />
        </AppCtx>
      ),
      { width: 10, height: 5 },
    );

    await renderOnce();
    expect(resolved).toBe(value);
  });
});
