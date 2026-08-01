import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type {
  IConfigService,
  IProviderRuntime,
  ModelCatalogConfig,
} from "@moonshot-ai/agent-core-v2";

import { ModelCatalogRefreshScheduler } from "../src/services/modelCatalog/modelCatalogRefreshScheduler";
import type { ServerLogger } from "../src/services/pinoLoggerService";

const EMPTY_RESULT = { aborted: false, errors: new Map() };

function makeCatalog(refresh = vi.fn(async () => EMPTY_RESULT)) {
  return { refresh } as unknown as IProviderRuntime;
}

function makeConfig(catalogConfig?: ModelCatalogConfig): IConfigService {
  return {
    ready: Promise.resolve(),
    get: vi.fn((domain: string) => (domain === "modelCatalog" ? catalogConfig : undefined)),
  } as unknown as IConfigService;
}

function makeLogger(): Pick<ServerLogger, "info" | "warn"> {
  return {
    info: vi.fn(),
    warn: vi.fn(),
  };
}

describe("ModelCatalogRefreshScheduler", () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
    vi.unstubAllEnvs();
  });

  it("refreshes on start and then on the configured interval", async () => {
    const catalog = makeCatalog();
    const scheduler = new ModelCatalogRefreshScheduler(catalog, makeConfig(), makeLogger(), {});

    await scheduler.start();
    await vi.waitFor(() => {
      expect(catalog.refresh).toHaveBeenCalledTimes(1);
    });
    expect(catalog.refresh).toHaveBeenCalledWith({ allowNetwork: true });

    await vi.advanceTimersByTimeAsync(6 * 60 * 60 * 1000);
    expect(catalog.refresh).toHaveBeenCalledTimes(2);

    await scheduler.dispose();
    await vi.advanceTimersByTimeAsync(6 * 60 * 60 * 1000);
    expect(catalog.refresh).toHaveBeenCalledTimes(2);
  });

  it("honors env overrides for interval and refresh-on-start", async () => {
    const catalog = makeCatalog();
    const scheduler = new ModelCatalogRefreshScheduler(catalog, makeConfig(), makeLogger(), {
      DIMI_CODE_MODEL_CATALOG_REFRESH_INTERVAL_MS: "1000",
      DIMI_CODE_MODEL_CATALOG_REFRESH_ON_START: "0",
    });

    await scheduler.start();
    await vi.advanceTimersByTimeAsync(999);
    expect(catalog.refresh).not.toHaveBeenCalled();

    await vi.advanceTimersByTimeAsync(1);
    expect(catalog.refresh).toHaveBeenCalledTimes(1);
  });

  it("reads interval and refresh-on-start from the modelCatalog config section", async () => {
    const catalog = makeCatalog();
    const scheduler = new ModelCatalogRefreshScheduler(
      catalog,
      makeConfig({ refreshIntervalMs: 1000, refreshOnStart: false }),
      makeLogger(),
      {},
    );

    await scheduler.start();
    // refreshOnStart=false → no startup refresh.
    await vi.advanceTimersByTimeAsync(999);
    expect(catalog.refresh).not.toHaveBeenCalled();
    // interval=1000 → first interval refresh at 1000ms.
    await vi.advanceTimersByTimeAsync(1);
    expect(catalog.refresh).toHaveBeenCalledTimes(1);
  });

  it("lets env override the modelCatalog config section", async () => {
    const catalog = makeCatalog();
    const scheduler = new ModelCatalogRefreshScheduler(
      catalog,
      makeConfig({ refreshIntervalMs: 6 * 60 * 60 * 1000, refreshOnStart: true }),
      makeLogger(),
      {
        DIMI_CODE_MODEL_CATALOG_REFRESH_ON_START: "0",
        DIMI_CODE_MODEL_CATALOG_REFRESH_INTERVAL_MS: "1000",
      },
    );

    await scheduler.start();
    await vi.advanceTimersByTimeAsync(999);
    expect(catalog.refresh).not.toHaveBeenCalled();
    await vi.advanceTimersByTimeAsync(1);
    expect(catalog.refresh).toHaveBeenCalledTimes(1);
  });

  it("disables the schedule when the config interval is 0", async () => {
    const catalog = makeCatalog();
    const scheduler = new ModelCatalogRefreshScheduler(
      catalog,
      makeConfig({ refreshIntervalMs: 0, refreshOnStart: false }),
      makeLogger(),
      {},
    );

    await scheduler.start();
    await vi.advanceTimersByTimeAsync(60 * 1000);
    expect(catalog.refresh).not.toHaveBeenCalled();
  });

  it("swallows refresh errors so a failing tick does not break the schedule", async () => {
    const refresh = vi.fn().mockRejectedValue(new Error("network down"));
    const catalog = makeCatalog(refresh);
    const logger = makeLogger();
    const scheduler = new ModelCatalogRefreshScheduler(catalog, makeConfig(), logger, {
      DIMI_CODE_MODEL_CATALOG_REFRESH_INTERVAL_MS: "1000",
      DIMI_CODE_MODEL_CATALOG_REFRESH_ON_START: "false",
    });

    await scheduler.start();
    await vi.advanceTimersByTimeAsync(1000);
    await vi.advanceTimersByTimeAsync(1000);

    expect(refresh).toHaveBeenCalledTimes(2);
    expect(logger.warn).toHaveBeenCalled();
  });
});
