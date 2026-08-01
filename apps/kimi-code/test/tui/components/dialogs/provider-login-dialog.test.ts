/**
 * Scenario: one provider-owned TUI dialog drives every authentication interaction.
 * Responsibilities: retain authorization context, collect prompts safely, and cancel cleanly.
 * Wiring: real dialog and input/list components with render/cancel callback spies.
 * Run: vp exec vitest run test/tui/components/dialogs/provider-login-dialog.test.ts
 */
import { describe, expect, it, vi } from 'vitest';

import { ProviderLoginDialogComponent } from '#/tui/components/dialogs/provider-login-dialog';

const ANSI_SGR = /\u001B\[[0-9;]*m/g;
const strip = (text: string): string => text.replaceAll(ANSI_SGR, '');

function createDialog() {
  const requestRender = vi.fn();
  const onCancel = vi.fn();
  const dialog = new ProviderLoginDialogComponent({
    providerName: 'OpenAI Codex',
    methodLabel: 'Sign in with ChatGPT',
    requestRender,
    onCancel,
  });
  dialog.focused = true;
  return { dialog, onCancel, requestRender };
}

describe('ProviderLoginDialogComponent', () => {
  it('renders provider-owned device authorization without Kimi-specific copy', () => {
    const { dialog } = createDialog();
    dialog.notify({
      type: 'device_code',
      userCode: 'ABCD-EFGH',
      verificationUri: 'https://example.test/device',
    });

    const output = strip(dialog.render(100).join('\n'));
    expect(output).toContain('Connect to OpenAI Codex');
    expect(output).toContain('Sign in with ChatGPT');
    expect(output).toContain('https://example.test/device');
    expect(output).toContain('ABCD-EFGH');
    expect(output).not.toContain('Sign in to Kimi Code');
  });

  it('keeps earlier authorization details visible while a later prompt is active', () => {
    const { dialog } = createDialog();
    dialog.notify({
      type: 'auth_url',
      url: 'https://example.test/authorize',
      instructions: 'Authorize the provider in your browser.',
    });
    void dialog.prompt({
      type: 'text',
      message: 'Paste the callback URL:',
      placeholder: 'https://localhost/callback',
    });

    const output = strip(dialog.render(100).join('\n'));
    expect(output).toContain('https://example.test/authorize');
    expect(output).toContain('Authorize the provider in your browser.');
    expect(output).toContain('Paste the callback URL:');
  });

  it('resolves a submitted secret without rendering its raw value', async () => {
    const { dialog } = createDialog();
    const answer = dialog.prompt({ type: 'secret', message: 'Enter API key:' });
    for (const character of 'YOUR_API_KEY') dialog.handleInput(character);

    const output = strip(dialog.render(100).join('\n'));
    expect(output).not.toContain('YOUR_API_KEY');
    expect(output).toContain('••••');
    dialog.handleInput('\r');
    await expect(answer).resolves.toBe('YOUR_API_KEY');
  });

  it('supports provider-owned select prompts', async () => {
    const { dialog } = createDialog();
    const answer = dialog.prompt({
      type: 'select',
      message: 'Choose an organization:',
      options: [
        { id: 'alpha', label: 'Alpha' },
        { id: 'beta', label: 'Beta', description: 'Secondary organization' },
      ],
    });
    dialog.handleInput('\u001B[B');
    dialog.handleInput('\r');

    await expect(answer).resolves.toBe('beta');
  });

  it('aborts the active prompt when the user cancels', async () => {
    const { dialog, onCancel } = createDialog();
    const answer = dialog.prompt({ type: 'text', message: 'Enter value:' });
    dialog.handleInput('\u001B');

    expect(onCancel).toHaveBeenCalledOnce();
    await expect(answer).rejects.toMatchObject({ name: 'AbortError' });
  });
});
