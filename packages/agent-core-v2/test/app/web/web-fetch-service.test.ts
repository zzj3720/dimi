import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { DisposableStore } from "#/_base/di/lifecycle";
import { createServices, type TestInstantiationService } from "#/_base/di/test";
import { IConfigService } from "#/app/config/config";
import { IProviderRuntime } from "#/app/providerRuntime/providerRuntime";
import type { Model } from "#/app/providerRuntime/types";
import { SERVICES_SECTION, type ServicesConfig } from "#/app/config/servicesConfig";
import { LocalFetchURLProvider } from "#/app/web/providers/local-fetch-url";
import { MoonshotFetchURLProvider } from "#/app/web/providers/moonshot-fetch-url";
import { IWebFetchService } from "#/app/web/web";
import { WebFetchService } from "#/app/web/webService";
import { IHostRequestHeaders } from "#/app/providerRuntime/hostRequestHeaders";

describe("WebFetchService", () => {
  let disposables: DisposableStore;
  let ix: TestInstantiationService;
  let servicesConfig: ServicesConfig | undefined;
  let dimiModel: Model | undefined;
  let dimiToken: string | undefined;

  beforeEach(() => {
    disposables = new DisposableStore();
    servicesConfig = undefined;
    dimiModel = undefined;
    dimiToken = undefined;
    ix = createServices(disposables, {
      additionalServices: (reg) => {
        reg.definePartialInstance(IProviderRuntime, {
          ready: Promise.resolve(),
          getModels: ((provider?: string) =>
            provider === "kimi-coding" && dimiModel !== undefined
              ? [dimiModel]
              : []) as IProviderRuntime["getModels"],
          getAuth: (() =>
            Promise.resolve(
              dimiToken === undefined ? undefined : { auth: { apiKey: dimiToken }, source: "test" },
            )) as IProviderRuntime["getAuth"],
        });
        reg.definePartialInstance(IHostRequestHeaders, {
          headers: {
            "User-Agent": "dimi-cli/test",
            "X-Msh-Device-Id": "device-test",
          },
        });
        reg.definePartialInstance(IConfigService, {
          get: ((domain: string) =>
            domain === SERVICES_SECTION ? servicesConfig : undefined) as IConfigService["get"],
        });
        reg.define(IWebFetchService, WebFetchService);
      },
    });
  });

  afterEach(() => {
    disposables.dispose();
    vi.unstubAllGlobals();
  });

  function fetcher(): ReturnType<IWebFetchService["getUrlFetcher"]> {
    return ix.get(IWebFetchService).getUrlFetcher();
  }

  it("uses the local fetcher when neither config nor the Dimi provider is available", () => {
    expect(fetcher()).toBeInstanceOf(LocalFetchURLProvider);
  });

  it("builds a Moonshot fetcher from the authenticated Dimi runtime model", async () => {
    dimiModel = model("https://api.example.com/v1");
    dimiToken = "access-token";
    const fetchMock = vi.fn().mockResolvedValue({
      status: 200,
      text: async () => "page body",
    });
    vi.stubGlobal("fetch", fetchMock);

    const result = await fetcher().fetch("https://example.com/page");

    expect(result).toEqual({ content: "page body", kind: "extracted" });
    const [url, init] = fetchMock.mock.calls[0] as [string, RequestInit];
    expect(url).toBe("https://api.example.com/v1/fetch");
    expect(init.headers).toMatchObject({
      Authorization: "Bearer access-token",
      "User-Agent": "dimi-cli/test",
      "X-Msh-Device-Id": "device-test",
    });
  });

  it("builds a Moonshot fetcher from services.moonshot_fetch config", async () => {
    servicesConfig = {
      moonshotFetch: {
        baseUrl: "https://fetch.example.com/fetch",
        apiKey: "fetch-key",
        customHeaders: { "X-Config": "1" },
      },
    };
    const fetchMock = vi.fn().mockResolvedValue({
      status: 200,
      text: async () => "page body",
    });
    vi.stubGlobal("fetch", fetchMock);

    expect(fetcher()).toBeInstanceOf(MoonshotFetchURLProvider);
    await fetcher().fetch("https://example.com/page");

    const [url, init] = fetchMock.mock.calls[0] as [string, RequestInit];
    expect(url).toBe("https://fetch.example.com/fetch");
    expect(init.headers).toMatchObject({
      Authorization: "Bearer fetch-key",
      "X-Config": "1",
    });
  });

  it("prefers services.moonshot_fetch over the Dimi runtime model", () => {
    dimiModel = model("https://managed.example.com/v1");
    servicesConfig = {
      moonshotFetch: {
        baseUrl: "https://config.example.com/fetch",
        apiKey: "config-key",
      },
    };

    expect(fetcher()).toBeInstanceOf(MoonshotFetchURLProvider);
  });

  it("uses the local fetcher when services.moonshot_fetch lacks a base URL", () => {
    servicesConfig = { moonshotFetch: { apiKey: "fetch-key" } };
    expect(fetcher()).toBeInstanceOf(LocalFetchURLProvider);
  });
});

function model(baseUrl: string): Model {
  return {
    id: "kimi-for-coding",
    name: "Dimi for Coding",
    api: "openai-completions",
    provider: "kimi-coding",
    baseUrl,
    reasoning: true,
    input: ["text", "image"],
    cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 },
    contextWindow: 262_144,
    maxTokens: 32_768,
  };
}
