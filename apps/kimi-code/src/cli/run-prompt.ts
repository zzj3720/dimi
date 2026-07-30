/**
 * Native print-mode entry and process-lifecycle helpers.
 */

import type { CLIOptions } from './options';
import { runPrint, type PromptRunIO } from './run-print';

export {
  configuredModel,
  installPromptTerminationCleanup,
  raceWithTimeout,
  requireConfiguredModel,
  signalExitCode,
} from './run-print';
export type { PromptProcess, PromptRunIO } from './run-print';

export async function runPrompt(
  opts: CLIOptions,
  version: string,
  io: PromptRunIO = {},
): Promise<void> {
  await runPrint(opts, version, io);
}
