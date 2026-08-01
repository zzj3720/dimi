/**
 * `webSearch` domain — `IWebSearchProviderService` implementation.
 *
 * Resolves the `WebSearch` backend from two sources, in precedence order:
 * (1) an explicit `[services.moonshot_search]` config section (read through
 * `config`, mirroring v1 where that section is the single authoritative
 * web-search source) — built with its API key and the active Dimi credential;
 * and (2) the authenticated `kimi-coding` provider, whose base URL is derived
 * from the runtime model catalog. The explicit config wins. Both use the
 * host's Dimi identity headers (`IHostRequestHeaders`,
 * mirroring v1's `dimiRequestHeaders`) as default headers. When neither source
 * is configured it yields `undefined` so the contributed `WebSearch` tool
 * stays hidden. Owns no tool registration — the `WebSearch` tool contributes
 * itself via `registerAgentToolService(...)` and reads this service from the
 * Agent-scope accessor.
 * Tests and hosts that need a custom backend bind `IWebSearchProviderService`
 * directly. Bound at App scope.
 */

import { LifecycleScope, ScopeActivation, registerScopedService } from "#/_base/di/scope";
import { IConfigService } from "#/app/config/config";
import { IProviderRuntime } from "#/app/providerRuntime/providerRuntime";
import { IHostRequestHeaders } from "#/app/providerRuntime/hostRequestHeaders";

import { SERVICES_SECTION, type ServicesConfig } from "#/app/config/servicesConfig";
import { MoonshotWebSearchProvider } from "./providers/moonshot-web-search";
import { IWebSearchProviderService, type WebSearchProvider } from "./webSearch";

export class WebSearchProviderService implements IWebSearchProviderService {
  declare readonly _serviceBrand: undefined;

  constructor(
    @IProviderRuntime private readonly runtime: IProviderRuntime,
    @IHostRequestHeaders private readonly hostHeaders: IHostRequestHeaders,
    @IConfigService private readonly config: IConfigService,
  ) {}

  getWebSearchProvider(): WebSearchProvider | undefined {
    return this.fromServicesConfig() ?? this.fromProviderAuth();
  }

  private fromServicesConfig(): WebSearchProvider | undefined {
    const search = this.config.get<ServicesConfig>(SERVICES_SECTION)?.moonshotSearch;
    if (search?.baseUrl === undefined) {
      return undefined;
    }
    return new MoonshotWebSearchProvider({
      baseUrl: search.baseUrl,
      tokenProvider: this.tokenProvider(),
      apiKey: nonEmptyString(search.apiKey),
      defaultHeaders: { ...this.hostHeaders.headers },
      customHeaders: search.customHeaders,
    });
  }

  private fromProviderAuth(): WebSearchProvider | undefined {
    const model = this.runtime.getModels("kimi-coding")[0];
    if (model === undefined) return undefined;
    const baseUrl = `${model.baseUrl.replace(/\/+$/, "")}/search`;
    return new MoonshotWebSearchProvider({
      baseUrl,
      tokenProvider: this.tokenProvider(),
      defaultHeaders: { ...this.hostHeaders.headers },
    });
  }

  private tokenProvider() {
    return {
      getAccessToken: async (): Promise<string> => {
        const auth = (await this.runtime.getAuth("kimi-coding"))?.auth;
        const token =
          auth?.apiKey ??
          Object.entries(auth?.headers ?? {})
            .find(([name]) => name.toLowerCase() === "authorization")?.[1]
            ?.match(/^Bearer\s+(.+)$/iu)?.[1];
        if (token === undefined) throw new Error("Dimi provider is not authenticated.");
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
  IWebSearchProviderService,
  WebSearchProviderService,
  ScopeActivation.OnScopeCreated,
  "auth",
);
