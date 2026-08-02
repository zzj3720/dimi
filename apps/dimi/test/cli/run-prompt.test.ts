/**
 * Scenario: print-mode entrypoint and process-lifecycle helpers.
 * Responsibilities: the CLI delegates to the sole print runtime and cleans up boundedly.
 * Wiring: the native print runner and process signals are local test boundaries.
 * Run: pnpm -C apps/dimi exec vitest run test/cli/run-prompt.test.ts
 */

import { afterEach, describe, expect, it, vi } from 'vitest';

import {
  configuredModel,
  qualifiedDefaultModel,
  installPromptTerminationCleanup,
  raceWithTimeout,
  requireConfiguredModel,
  runPrompt,
  signalExitCode,
  type PromptRunIO,
} from '#/cli/run-prompt';

const mocks = vi.hoisted(() => ({ runPrint: vi.fn() }));

vi.mock('../../src/cli/run-print', async (importOriginal) => ({
  ...(await importOriginal<typeof import('../../src/cli/run-print')>()),
  runPrint: mocks.runPrint,
}));

const options = {
  session: undefined,
  continue: false,
  yolo: false,
  auto: false,
  plan: false,
  model: undefined,
  outputFormat: undefined,
  prompt: 'say hello',
  skillsDirs: [],
      legacy: false,
  agent: undefined,
  agentFiles: [],
  addDirs: [],
};

afterEach(() => {
  vi.clearAllMocks();
  vi.useRealTimers();
});

describe('runPrompt', () => {
  it('delegates every invocation to the sole print runtime', async () => {
    const io: PromptRunIO = {
      stdout: { write: vi.fn(() => true) },
      stderr: { write: vi.fn(() => true) },
    };

    await runPrompt(options, '1.2.3-test', io);

    expect(mocks.runPrint).toHaveBeenCalledOnce();
    expect(mocks.runPrint).toHaveBeenCalledWith(options, '1.2.3-test', io);
  });
});

describe('prompt lifecycle helpers', () => {
  it('selects the first configured model and rejects an empty model list', () => {
    expect(configuredModel(undefined, ' ', 'grok-code')).toBe('grok-code');
    expect(requireConfiguredModel(undefined, 'grok-code')).toBe('grok-code');
    expect(() => requireConfiguredModel(undefined, ' ')).toThrow('No model configured');
  });

  it('qualifies the persisted model with its provider for the runtime lookup', () => {
    expect(qualifiedDefaultModel('xai', 'grok-4.5')).toBe('xai/grok-4.5');
    expect(qualifiedDefaultModel('xai', 'xai/grok-4.5')).toBe('xai/grok-4.5');
    expect(qualifiedDefaultModel('openrouter', 'anthropic/claude-sonnet-4')).toBe(
      'openrouter/anthropic/claude-sonnet-4',
    );
  });

  it('surfaces an early cleanup failure', async () => {
    await expect(raceWithTimeout(Promise.reject(new Error('close failed')), 100)).rejects.toThrow(
      'close failed',
    );
  });

  it('stops waiting at the cleanup timeout and consumes a later rejection', async () => {
    vi.useFakeTimers();
    const cleanup = new Promise<void>((_, reject) => {
      setTimeout(() => reject(new Error('late close')), 200);
    });
    const result = raceWithTimeout(cleanup, 100);

    await vi.advanceTimersByTimeAsync(100);
    await expect(result).resolves.toBeUndefined();
    await vi.advanceTimersByTimeAsync(100);
  });

  it('cleans up once before exiting for a termination signal', async () => {
    const listeners = new Map<NodeJS.Signals, () => Promise<void>>();
    const promptProcess = {
      once: vi.fn((signal: NodeJS.Signals, listener: () => Promise<void>) => {
        listeners.set(signal, listener);
      }),
      off: vi.fn((signal: NodeJS.Signals) => {
        listeners.delete(signal);
      }),
      exit: vi.fn(),
    };
    const cleanup = vi.fn(async () => {});
    const uninstall = installPromptTerminationCleanup(promptProcess, cleanup);

    await listeners.get('SIGINT')?.();

    expect(cleanup).toHaveBeenCalledOnce();
    expect(promptProcess.exit).toHaveBeenCalledWith(130);
    uninstall();
    expect(listeners.size).toBe(0);
  });

  it('maps supported termination signals to conventional exit codes', () => {
    expect(signalExitCode('SIGINT')).toBe(130);
    expect(signalExitCode('SIGHUP')).toBe(129);
    expect(signalExitCode('SIGTERM')).toBe(143);
  });
});
