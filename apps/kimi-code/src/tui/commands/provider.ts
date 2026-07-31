import { formatErrorMessage } from '../utils/event-payload';
import { handleLoginCommand } from './auth';
import type { SlashCommandHost } from './dispatch';
import { promptProviderSelection } from './prompts';

/**
 * Show the runtime-owned provider catalog. Credentials are managed through
 * `/login` and `/logout`; `/provider refresh` only refreshes dynamic model
 * lists and never writes provider definitions into config.toml.
 */
export async function handleProviderCommand(host: SlashCommandHost, args: string): Promise<void> {
  const command = args.trim();
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
