/**
 * Scenario: provider authentication selection in the TUI.
 * Responsibilities: filter by authentication type, expose credential status, and return a choice.
 * Wiring: real selector components with callback spies only.
 * Run: vp exec vitest run test/tui/components/dialogs/provider-auth-selector.test.ts
 */
import type { ProviderAuthState } from '@moonshot-ai/kimi-code-sdk';
import { describe, expect, it, vi } from 'vitest';

import {
  AuthTypeSelectorComponent,
  ProviderAuthSelectorComponent,
} from '#/tui/components/dialogs/provider-auth-selector';

const ANSI_SGR = /\u001B\[[0-9;]*m/g;
const strip = (text: string): string => text.replaceAll(ANSI_SGR, '');

const providers: readonly ProviderAuthState[] = [
  {
    id: 'openai-codex',
    name: 'OpenAI Codex',
    configured: true,
    credentialType: 'oauth',
    source: 'Stored OAuth',
    methods: [{ type: 'oauth', name: 'OpenAI', label: 'Sign in with ChatGPT' }],
  },
  {
    id: 'openai',
    name: 'OpenAI',
    configured: true,
    credentialType: 'api_key',
    source: 'OPENAI_API_KEY',
    methods: [{ type: 'api_key', name: 'OpenAI API key', label: 'OpenAI API key' }],
  },
  {
    id: 'anthropic',
    name: 'Anthropic',
    configured: false,
    methods: [{ type: 'api_key', name: 'Anthropic API key', label: 'Anthropic API key' }],
  },
];

describe('ProviderAuthSelectorComponent', () => {
  it('shows only API-key providers with their effective credential status', () => {
    const selector = new ProviderAuthSelectorComponent({
      title: 'Select an API key provider',
      providers,
      authType: 'api_key',
      onSelect: vi.fn(),
      onCancel: vi.fn(),
    });

    const output = selector.render(120).map(strip).join('\n');
    expect(output).toContain('OpenAI');
    expect(output).toContain('OPENAI_API_KEY');
    expect(output).toContain('Anthropic');
    expect(output).toContain('not connected');
    expect(output).not.toContain('OpenAI Codex');
  });

  it('searches provider names, ids, methods, and status', () => {
    const selector = new ProviderAuthSelectorComponent({
      title: 'Select a provider',
      providers,
      onSelect: vi.fn(),
      onCancel: vi.fn(),
    });
    for (const character of 'codex') selector.handleInput(character);

    const output = selector.render(120).map(strip).join('\n');
    expect(output).toContain('OpenAI Codex');
    expect(output).not.toContain('Anthropic');
  });

  it('presents each authentication family as a separate first-class choice', () => {
    const onSelect = vi.fn();
    const selector = new AuthTypeSelectorComponent({
      methods: providers.flatMap((provider) => provider.methods),
      onSelect,
      onCancel: vi.fn(),
    });
    const output = selector.render(120).map(strip).join('\n');

    expect(output).toContain('Sign in with an account');
    expect(output).toContain('Sign in with an API key');
    selector.handleInput('\r');
    expect(onSelect).toHaveBeenCalledWith('oauth');
  });
});
