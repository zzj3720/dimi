import { formatErrorMessage } from '../utils/event-payload';
import { readFile } from 'node:fs/promises';
import { parseJsonc } from '@dimi-agent/dimi-sdk';
import { CustomProviderDialogComponent } from '../components/dialogs/custom-provider-dialog';
import { handleLoginCommand } from './auth';
import type { SlashCommandHost } from './dispatch';
import { promptProviderSelection } from './prompts';

/**
 * Show the runtime-owned provider catalog. Credentials are managed through
 * `/login` and `/logout`; custom provider definitions are persisted by the
 * runtime in models.json, never in config.toml.
 */
export async function handleProviderCommand(host: SlashCommandHost, args: string): Promise<void> {
  const command = args.trim();
  if (command.toLowerCase() === 'add') {
    const provider = await promptCustomProvider(host);
    if (provider === undefined) return;
    await host.harness.auth.upsertCustomProvider(provider);
    await host.authFlow.refreshAvailableModels();
    host.showStatus(`Added ${provider.name}. Run /login ${provider.id} to connect it.`, 'success');
    return;
  }
  if (command.toLowerCase().startsWith('import ')) {
    const path = command.slice('import '.length).trim();
    if (path.length === 0) {
      host.showError('Usage: /provider import <definition.json>');
      return;
    }
    try {
      const definitions = await readCustomProviderDefinitions(path);
      for (const definition of definitions) await host.harness.auth.upsertCustomProvider(definition);
      await host.authFlow.refreshAvailableModels();
      host.showStatus(`Imported ${String(definitions.length)} provider definition(s).`, 'success');
    } catch (error) {
      host.showError(`Failed to import providers: ${formatErrorMessage(error)}`);
    }
    return;
  }
  if (command.toLowerCase().startsWith('remove ')) {
    const id = command.slice('remove '.length).trim();
    if (id.length === 0) {
      host.showError('Usage: /provider remove <provider-id>');
      return;
    }
    await host.harness.auth.deleteCustomProvider(id);
    await host.authFlow.refreshAvailableModels();
    host.showStatus(`Removed ${id}.`, 'success');
    return;
  }
  if (command.toLowerCase() === 'refresh') {
    const result = await host.authFlow.refreshProviderModels();
    for (const failure of result.failed) {
      host.showStatus(`Skipped ${failure.provider}: ${failure.reason}`, 'warning');
    }
    host.showStatus(
      `Provider models refreshed: ${String(result.changed.length)} changed, ${String(
        result.unchanged.length,
      )} unchanged.`,
      result.failed.length === 0 ? 'success' : 'warning',
    );
    return;
  }

  try {
    const providers = await host.harness.auth.providers();
    if (command.length > 0) {
      await handleLoginCommand(host, command);
      return;
    }
    const providerId = await promptProviderSelection(host, providers);
    if (providerId !== undefined) await handleLoginCommand(host, providerId);
  } catch (error) {
    host.showError(`Failed to manage providers: ${formatErrorMessage(error)}`);
  }
}

async function promptCustomProvider(host: SlashCommandHost) {
  const apis = host.harness.auth.providerApis();
  return new Promise<import('@dimi-agent/dimi-sdk').CustomProviderInput | undefined>((resolve) => {
    host.mountEditorReplacement(
      new CustomProviderDialogComponent(apis, (result) => {
        host.restoreEditor();
        resolve(result.kind === 'ok' ? result.provider : undefined);
      }),
    );
  });
}

async function readCustomProviderDefinitions(
  path: string,
): Promise<readonly import('@dimi-agent/dimi-sdk').CustomProviderInput[]> {
  const value = parseJsonc(await readFile(path, 'utf8'));
  const root = typeof value === 'object' && value !== null ? value as Record<string, unknown> : undefined;
  const providers = root?.['providers'];
  if (providers !== undefined && typeof providers === 'object' && providers !== null && !Array.isArray(providers)) {
    return Object.entries(providers).map(([id, definition]) => ({
      ...(definition as import('@dimi-agent/dimi-sdk').CustomProviderInput),
      id,
    }));
  }
  if (root?.['id'] !== undefined) {
    return [root as unknown as import('@dimi-agent/dimi-sdk').CustomProviderInput];
  }
  throw new Error('Expected a provider definition or { providers: { ... } }.');
}
