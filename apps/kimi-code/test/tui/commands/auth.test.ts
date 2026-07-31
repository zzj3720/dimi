/**
 * Scenario: TUI login orchestration through the provider authentication facade.
 * Responsibilities: route direct and selector-based login, then persist the chosen model.
 * Wiring: real TUI components with the SDK facade and host effects stubbed at their boundaries.
 * Run: vp exec vitest run test/tui/commands/auth.test.ts
 */
import type {
  AuthInteraction,
  AuthType,
  ProviderAuthState,
  ProviderModel,
} from '@moonshot-ai/kimi-code-sdk';
import type { Component, Focusable } from '@moonshot-ai/pi-tui';
import { afterEach, describe, expect, it, vi } from 'vitest';

import { handleLoginCommand } from '#/tui/commands/auth';
import type { SlashCommandHost } from '#/tui/commands/dispatch';
import { AuthTypeSelectorComponent } from '#/tui/components/dialogs/provider-auth-selector';
import { ProviderLoginDialogComponent } from '#/tui/components/dialogs/provider-login-dialog';
import { ModelSelectorComponent } from '#/tui/components/dialogs/model-selector';
import { openUrl } from '#/utils/open-url';

vi.mock('#/utils/open-url', () => ({ openUrl: vi.fn() }));

afterEach(() => {
  vi.clearAllMocks();
});

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

function makeHost(overrides: {
  providers: readonly ProviderAuthState[];
  login: (provider: string, method: AuthType, interaction: AuthInteraction) => Promise<unknown>;
  models?: readonly ProviderModel[];
}) {
  type InteractiveComponent = Component & Focusable & { handleInput(data: string): void };
  let mounted: InteractiveComponent | undefined;
  const appState = {
    model: '',
    thinkingEffort: 'off',
    availableModels: {},
    availableProviders: {},
  };
  const harness = {
    auth: {
      providers: vi.fn(async () => overrides.providers),
      login: vi.fn(overrides.login),
      models: vi.fn(async () => overrides.models ?? [model]),
    },
    setConfig: vi.fn(async () => ({})),
  };
  const host = {
    state: {
      ui: { requestRender: vi.fn() },
      appState,
    },
    session: undefined,
    harness,
    cancelInFlight: undefined,
    mountEditorReplacement: vi.fn((component: Component & Focusable) => {
      mounted = component as InteractiveComponent;
      component.focused = true;
    }),
    restoreEditor: vi.fn(() => {
      mounted = undefined;
    }),
    authFlow: {
      refreshAvailableModels: vi.fn(async () => {}),
      activateModelAfterLogin: vi.fn(async () => {}),
    },
    setAppState: vi.fn((patch) => Object.assign(appState, patch)),
    showStatus: vi.fn(),
    showError: vi.fn(),
    track: vi.fn(),
  } as unknown as SlashCommandHost;
  return {
    host,
    harness,
    current: () => mounted,
  };
}

describe('TUI provider login', () => {
  it('routes /login <provider> directly into a provider-owned OAuth dialog', async () => {
    let releaseLogin: (() => void) | undefined;
    const loginGate = new Promise<void>((resolve) => {
      releaseLogin = resolve;
    });
    const { host, harness, current } = makeHost({
      providers: [
        {
          id: 'openai-codex',
          name: 'OpenAI Codex',
          configured: false,
          methods: [{ type: 'oauth', name: 'OpenAI', label: 'Sign in with ChatGPT' }],
        },
      ],
      login: async (_provider, _method, interaction) => {
        interaction.notify({
          type: 'device_code',
          userCode: 'ABCD-EFGH',
          verificationUri: 'https://example.test/device',
        });
        await loginGate;
      },
    });

    const login = handleLoginCommand(host, 'openai-codex');
    await vi.waitFor(() => {
      expect(current()).toBeInstanceOf(ProviderLoginDialogComponent);
    });
    const output = current()!.render(120).join('\n');
    expect(output).toContain('OpenAI Codex');
    expect(output).toContain('ABCD-EFGH');
    expect(vi.mocked(openUrl)).toHaveBeenCalledWith('https://example.test/device');

    releaseLogin?.();
    await vi.waitFor(() => {
      expect(current()).toBeInstanceOf(ModelSelectorComponent);
    });
    current()!.handleInput('\r');
    await login;

    expect(harness.auth.login).toHaveBeenCalledWith(
      'openai-codex',
      'oauth',
      expect.objectContaining({ signal: expect.any(AbortSignal) }),
    );
    expect(harness.setConfig).toHaveBeenCalledWith({
      defaultProvider: 'openai-codex',
      defaultModel: 'gpt-5',
      thinking: { enabled: true, effort: 'medium' },
    });
  });

  it('persists the selected API-key provider model after the selector flow completes', async () => {
    const { host, harness, current } = makeHost({
      providers: [
        {
          id: 'openai-codex',
          name: 'OpenAI Codex',
          configured: false,
          methods: [{ type: 'oauth', name: 'OpenAI', label: 'Sign in with ChatGPT' }],
        },
        {
          id: 'openai',
          name: 'OpenAI',
          configured: false,
          methods: [{ type: 'api_key', name: 'OpenAI API key', label: 'OpenAI API key' }],
        },
      ],
      models: [{ ...model, provider: 'openai', api: 'openai-responses' }],
      login: async (_provider, _method, interaction) => {
        await interaction.prompt({ type: 'secret', message: 'Enter OpenAI API key:' });
      },
    });

    const login = handleLoginCommand(host);
    await vi.waitFor(() => {
      expect(current()).toBeInstanceOf(AuthTypeSelectorComponent);
    });
    current()!.handleInput('\u001B[B');
    current()!.handleInput('\r');

    await vi.waitFor(() => {
      expect(current()?.render(120).join('\n')).toContain('Select an API key provider');
    });
    current()!.handleInput('\r');

    await vi.waitFor(() => {
      expect(current()).toBeInstanceOf(ProviderLoginDialogComponent);
    });
    for (const character of 'YOUR_API_KEY') current()!.handleInput(character);
    current()!.handleInput('\r');

    await vi.waitFor(() => {
      expect(current()).toBeInstanceOf(ModelSelectorComponent);
    });
    current()!.handleInput('\r');
    await login;

    expect(harness.auth.login).toHaveBeenCalledWith(
      'openai',
      'api_key',
      expect.objectContaining({ prompt: expect.any(Function) }),
    );
    expect(harness.setConfig).toHaveBeenCalledWith({
      defaultProvider: 'openai',
      defaultModel: 'gpt-5',
      thinking: { enabled: true, effort: 'medium' },
    });
  });
});
