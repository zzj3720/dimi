/**
 * `webSearch` domain — authenticated web search seam.
 *
 * Owns the seam for the `WebSearch` backend. Web search needs an authenticated
 * Moonshot search provider, so it lives beside the provider runtime rather
 * than in the auth-independent `web` domain. `IWebSearchProviderService`
 * exposes the configured `WebSearchProvider` (or `undefined` when search is not
 * configured, in which case the `WebSearch` tool is not registered). The
 * default `WebSearchProviderService` builds the backend itself from the managed
 * Kimi OAuth provider's `oauth` ref (resolved through `IOAuthService`); tests
 * and hosts that need a custom backend bind `IWebSearchProviderService`
 * directly. Bound at App scope.
 */

import { createDecorator, type ServiceIdentifier } from "#/_base/di/instantiation";

export interface WebSearchResult {
  title: string;
  url: string;
  snippet: string;
  date?: string;
  siteName?: string;
}

export interface WebSearchProvider {
  search(
    query: string,
    options?: {
      toolCallId?: string;
      signal?: AbortSignal;
    },
  ): Promise<WebSearchResult[]>;
}

export interface IWebSearchProviderService {
  readonly _serviceBrand: undefined;

  getWebSearchProvider(): WebSearchProvider | undefined;
}

export const IWebSearchProviderService: ServiceIdentifier<IWebSearchProviderService> =
  createDecorator<IWebSearchProviderService>("webSearchProviderService");
