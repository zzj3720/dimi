/**
 * Shared KimiHarness boundary stubs for ACP tests.
 *
 * The adapter reads authentication state and the live provider catalog from
 * `harness.auth`; config only owns the selected model and thinking defaults.
 */

import type { ProviderModel } from '@moonshot-ai/kimi-code-sdk';

/** Stub `auth.status()` payload for an authenticated harness. */
export const AUTHED_STATUS = {
  providers: [{ providerName: 'test', hasToken: true }],
} as const;

/** Stub `auth.status()` payload for an unauthenticated harness. */
export const UNAUTHED_STATUS = {
  providers: [{ providerName: 'test', hasToken: false }],
} as const;

export interface TestProviderModel {
  readonly id: string;
  readonly provider?: string;
  readonly name?: string;
  readonly thinkingSupported?: boolean;
  readonly alwaysThinking?: boolean;
  readonly efforts?: readonly string[];
}

export function modelKey(id: string, provider = 'test'): string {
  return id.includes('/') ? id : `${provider}/${id}`;
}

/** Build the runtime-resolved model rows returned by `auth.models()`. */
export function makeProviderModels(entries: readonly TestProviderModel[]): readonly ProviderModel[] {
  return entries.map((entry) => {
    const separator = entry.id.indexOf('/');
    const provider =
      entry.provider ?? (separator === -1 ? 'test' : entry.id.slice(0, separator));
    const id = separator === -1 ? entry.id : entry.id.slice(separator + 1);
    const reasoning = entry.thinkingSupported === true || entry.alwaysThinking === true;
    const thinkingLevelMap =
      entry.efforts === undefined && entry.alwaysThinking !== true
        ? undefined
        : Object.fromEntries([
            ...(entry.alwaysThinking === true ? [['off', null] as const] : []),
            ...(entry.efforts ?? []).map((effort) => [effort, effort] as const),
          ]);
    return {
      id,
      name: entry.name ?? id,
      api: 'openai-completions',
      provider,
      baseUrl: 'https://api.example.test/v1',
      reasoning,
      input: ['text'],
      cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 },
      contextWindow: 128_000,
      maxTokens: 32_000,
      thinkingLevelMap,
    };
  });
}

/** Small auth facade stub; callers provide only the live catalog they need. */
export function makeAuth(
  options: {
    readonly authenticated?: boolean;
    readonly models?: readonly ProviderModel[];
  } = {},
) {
  const status = options.authenticated === false ? UNAUTHED_STATUS : AUTHED_STATUS;
  const models =
    options.models ??
    makeProviderModels([
      { id: 'kimi-coder', name: 'Kimi Coder', thinkingSupported: true },
      { id: 'kimi-plain', name: 'Kimi Plain' },
    ]);
  return {
    status: async () => status,
    models: async () => models,
  };
}
