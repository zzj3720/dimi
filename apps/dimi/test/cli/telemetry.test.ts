/**
 * Tests for the CLI telemetry bootstrap helpers, focusing on the
 * `dimi web` / `dimi server run` host wiring added in `cli/telemetry.ts`.
 */

import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  initializeTelemetry: vi.fn(),
  createDimiDeviceId: vi.fn(() => "device-123"),
  resolveDimiHome: vi.fn(() => "/home/.dimi"),
  resolveConfigPath: vi.fn(() => "/home/.dimi/config.toml"),
  readFileSync: vi.fn(() => 'default_model = "kimi-k2"\ntelemetry = true\n'),
  getCachedAccessToken: vi.fn(async () => "tok"),
}));

vi.mock("node:fs", () => ({ readFileSync: mocks.readFileSync }));

vi.mock("@dimi-agent/dimi-telemetry", () => ({
  initializeTelemetry: mocks.initializeTelemetry,
  setTelemetryContext: vi.fn(),
  track: vi.fn(),
  withTelemetryContext: vi.fn(),
}));

vi.mock("@dimi-agent/dimi-oauth", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@dimi-agent/dimi-oauth")>();
  return {
    ...actual,
    createDimiDeviceId: mocks.createDimiDeviceId,
  };
});

vi.mock("@dimi-agent/dimi-sdk", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@dimi-agent/dimi-sdk")>();
  return {
    ...actual,
    resolveDimiHome: mocks.resolveDimiHome,
    resolveConfigPath: mocks.resolveConfigPath,
    DimiAuthFacade: vi.fn(function () {
      return { getCachedAccessToken: mocks.getCachedAccessToken };
    }),
  };
});

describe("initializeServerTelemetry", () => {
  beforeEach(() => {
    mocks.initializeTelemetry.mockClear();
    mocks.readFileSync.mockReset();
    mocks.readFileSync.mockReturnValue('default_model = "kimi-k2"\ntelemetry = true\n');
  });

  it('configures the sink with ui_mode="web" and the CLI product identity', async () => {
    const { initializeServerTelemetry } = await import("#/cli/telemetry");
    const client = initializeServerTelemetry({ version: "1.2.3" });
    expect(mocks.initializeTelemetry).toHaveBeenCalledWith(
      expect.objectContaining({
        appName: "dimi-cli",
        version: "1.2.3",
        uiMode: "web",
        model: "kimi-k2",
        enabled: true,
        deviceId: "device-123",
        homeDir: "/home/.dimi",
      }),
    );
    // The returned client wraps the module functions so core + the host share
    // the same underlying client.
    expect(client).toEqual(
      expect.objectContaining({
        track: expect.any(Function),
        withContext: expect.any(Function),
        setContext: expect.any(Function),
      }),
    );
    // The first dynamic import pulls in the whole SDK/oauth chain (~3s idle,
    // more under full-suite transform contention) — give it headroom past the
    // 5s default timeout.
  }, 20000);

  it("disables telemetry when config.toml sets telemetry = false", async () => {
    mocks.readFileSync.mockReturnValue('default_model = "kimi-k2"\ntelemetry = false\n');
    const { initializeServerTelemetry } = await import("#/cli/telemetry");
    initializeServerTelemetry({ version: "1.2.3" });

    expect(mocks.initializeTelemetry).toHaveBeenCalledWith(
      expect.objectContaining({ enabled: false }),
    );
  });

  it("degrades to enabled with no model when config is unreadable", async () => {
    mocks.readFileSync.mockImplementation(() => {
      throw new Error("bad toml");
    });
    const { initializeServerTelemetry } = await import("#/cli/telemetry");
    initializeServerTelemetry({ version: "1.2.3" });

    expect(mocks.initializeTelemetry).toHaveBeenCalledWith(
      expect.objectContaining({ enabled: true, model: undefined }),
    );
  });
});
