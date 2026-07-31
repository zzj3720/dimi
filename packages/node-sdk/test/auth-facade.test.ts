import { describe, expect, it, vi } from 'vitest';

import { ProviderAuthFacade } from '#/auth';

function runtime() {
  const model = {
    provider: 'openai-codex',
    id: 'gpt-5',
    name: 'GPT-5',
    api: 'openai-codex-responses',
    baseUrl: 'https://example.test',
    reasoning: true,
    input: ['text'],
    cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 },
    contextWindow: 128_000,
    maxTokens: 16_000,
  } as const;
  return {
    ready: Promise.resolve(),
    refreshProviderDefinitions: vi.fn(async () => {}),
    listCustomProviders: vi.fn(async () => []),
    listCredentials: vi.fn(async () => [{ providerId: 'openai-codex', type: 'oauth' }]),
    getProviders: vi.fn(() => [
      {
        id: 'openai-codex',
        name: 'OpenAI Codex',
        auth: {
          oauth: {
            name: 'OpenAI',
            loginLabel: 'Sign in with ChatGPT',
            login: vi.fn(),
            refresh: vi.fn(),
            toAuth: vi.fn(),
          },
        },
      },
    ]),
    checkAuth: vi.fn(async () => ({ type: 'oauth', source: 'OAuth' })),
    getAvailable: vi.fn(async () => [model]),
    login: vi.fn(async () => ({ type: 'oauth' })),
    logout: vi.fn(async () => {}),
    refresh: vi.fn(async () => ({ aborted: false, errors: new Map() })),
    getAuth: vi.fn(async () => ({ auth: { apiKey: 'token' } })),
  };
}

describe('ProviderAuthFacade', () => {
  it('projects providers, credentials, and runtime auth methods', async () => {
    const target = new ProviderAuthFacade({ runtime: runtime() as never });
    await expect(target.providers()).resolves.toEqual([
      expect.objectContaining({
        id: 'openai-codex',
        configured: true,
        credentialType: 'oauth',
        source: 'OAuth',
        methods: [expect.objectContaining({ type: 'oauth', label: 'Sign in with ChatGPT' })],
      }),
    ]);
  });

  it('logs in, refreshes, and returns live provider models', async () => {
    const backing = runtime();
    const target = new ProviderAuthFacade({ runtime: backing as never });
    const interaction = { prompt: vi.fn(), notify: vi.fn() } as never;
    const result = await target.login('openai-codex', 'oauth', interaction);
    expect(backing.login).toHaveBeenCalledWith('openai-codex', 'oauth', interaction);
    expect(backing.refresh).toHaveBeenCalled();
    expect(result.models).toEqual([{ provider: 'openai-codex', id: 'gpt-5', name: 'GPT-5' }]);
  });

  it('patches a custom model without dropping its persisted request and capability fields', async () => {
    const backing = runtime();
    const existing = {
      id: 'local-model',
      name: 'Local',
      api: 'openai-responses',
      baseUrl: 'https://gateway.example.test/v1',
      contextWindow: 64_000,
      maxTokens: 8_000,
      reasoning: true,
      headers: { 'x-model': 'configured' },
      compat: { supportsStrictMode: true },
      thinkingLevelMap: { off: 'none', high: 'high' },
      cost: { input: 1, output: 2, cacheRead: 0, cacheWrite: 0 },
    };
    const custom = { id: 'local', api: 'openai-responses', baseUrl: 'https://gateway.example.test/v1', models: [existing] };
    const upsertCustomProvider = vi.fn(async () => {});
    Object.assign(backing, {
      listCustomProviders: vi.fn(async () => [custom]),
      upsertCustomProvider,
    });
    const target = new ProviderAuthFacade({ runtime: backing as never });

    await target.upsertCustomModel('local', { id: 'local-model', maxTokens: 16_000 });

    expect(upsertCustomProvider).toHaveBeenCalledWith({
      ...custom,
      models: [{ ...existing, maxTokens: 16_000 }],
    });
  });
});
