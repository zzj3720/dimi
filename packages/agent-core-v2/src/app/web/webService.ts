/**
 * `web` domain (L4) — `IWebFetchService` implementation.
 *
 * Yields the `UrlFetcher` the `FetchURL` tool uses, resolving the backend in
 * precedence order: (1) an explicit `[services.moonshot_fetch]` endpoint;
 * (2) the authenticated `kimi-coding` provider's managed fetch endpoint; and
 * (3) the built-in `LocalFetchURLProvider`. Remote fetchers receive the host
 * identity headers and fall back to the local fetcher on failure. Config and
 * provider auth are resolved lazily so edits and login state take effect
 * without rebuilding the service. Bound at App scope.
 */

import { LifecycleScope, ScopeActivation, registerScopedService } from "#/_base/di/scope";
import { IConfigService } from "#/app/config/config";
import { IProviderRuntime } from "#/app/providerRuntime/providerRuntime";
import { SERVICES_SECTION, type ServicesConfig } from "#/app/config/servicesConfig";
import { IHostRequestHeaders } from "#/app/providerRuntime/hostRequestHeaders";

import { LocalFetchURLProvider } from "./providers/local-fetch-url";
import { MoonshotFetchURLProvider } from "./providers/moonshot-fetch-url";
import type { UrlFetcher } from "./tools/fetch-url-types";
import { IWebFetchService } from "./web";

export class WebFetchService implements IWebFetchService {
  declare readonly _serviceBrand: undefined;
  private readonly localFetcher: UrlFetcher;

  constructor(
    @IProviderRuntime private readonly runtime: IProviderRuntime,
    @IHostRequestHeaders private readonly hostHeaders: IHostRequestHeaders,
    @IConfigService private readonly config: IConfigService,
  ) {
    this.localFetcher = new LocalFetchURLProvider();
  }

  getUrlFetcher(): UrlFetcher {
    return this.fromServicesConfig() ?? this.fromManagedOAuth() ?? this.localFetcher;
  }

  private fromServicesConfig(): UrlFetcher | undefined {
    const fetchConfig = this.config.get<ServicesConfig>(SERVICES_SECTION)?.moonshotFetch;
    if (fetchConfig?.baseUrl === undefined) {
      return undefined;
    }
    return new MoonshotFetchURLProvider({
      baseUrl: fetchConfig.baseUrl,
      tokenProvider: this.tokenProvider(),
      apiKey: nonEmptyString(fetchConfig.apiKey),
      defaultHeaders: { ...this.hostHeaders.headers },
      customHeaders: fetchConfig.customHeaders,
      localFallback: this.localFetcher,
    });
  }

  private fromManagedOAuth(): UrlFetcher | undefined {
    const model = this.runtime.getModels("kimi-coding")[0];
    if (model === undefined) return undefined;
    const baseUrl = `${model.baseUrl.replace(/\/+$/, "")}/fetch`;
    return new MoonshotFetchURLProvider({
      baseUrl,
      tokenProvider: this.tokenProvider(),
      defaultHeaders: { ...this.hostHeaders.headers },
      localFallback: this.localFetcher,
    });
  }

  private tokenProvider() {
    return {
      getAccessToken: async (): Promise<string> => {
        const token = (await this.runtime.getAuth("kimi-coding"))?.auth.apiKey;
        if (token === undefined) throw new Error("Kimi provider is not authenticated.");
        return token;
      },
    };
  }
}

function nonEmptyString(value: string | undefined): string | undefined {
  const trimmed = value?.trim();
  return trimmed === undefined || trimmed.length === 0 ? undefined : trimmed;
}

registerScopedService(
  LifecycleScope.App,
  IWebFetchService,
  WebFetchService,
  ScopeActivation.OnScopeCreated,
  "web",
);
