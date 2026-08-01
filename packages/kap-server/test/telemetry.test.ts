/**
 * Kap server telemetry contract — Dimi does not collect or upload telemetry,
 * so `initializeServerTelemetry` is a no-op: no appender is attached, no
 * device id is minted, and no network request is ever made.
 */

import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { initializeServerTelemetry, shutdownServerTelemetry } from "../src/services/telemetry";

describe("server telemetry no-op contract", () => {
  let home: string | undefined;

  beforeEach(async () => {
    home = await mkdtemp(join(tmpdir(), "dimi-server-telemetry-"));
  });

  afterEach(async () => {
    vi.unstubAllGlobals();
    if (home !== undefined) {
      await rm(home, { recursive: true, force: true });
      home = undefined;
    }
  });

  it("returns an empty handle and never attaches an appender", async () => {
    const telemetry = await initializeServerTelemetry({} as never, home as string);
    expect(telemetry.appender).toBeUndefined();
    expect(telemetry.registration).toBeUndefined();
    await expect(shutdownServerTelemetry(telemetry)).resolves.toBeUndefined();
  });

  it("does not perform network calls", async () => {
    const fetchSpy = vi.spyOn(globalThis, "fetch").mockRejectedValue(new Error("must not fetch"));
    const telemetry = await initializeServerTelemetry({} as never, home as string);
    await shutdownServerTelemetry(telemetry);
    expect(fetchSpy).not.toHaveBeenCalled();
    fetchSpy.mockRestore();
  });
});
