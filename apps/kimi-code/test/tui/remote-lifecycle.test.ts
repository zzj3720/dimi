import { beforeEach, describe, expect, it, vi } from 'vitest';

import { KimiTUI, type KimiTUIStartupInput } from '#/tui/kimi-tui';
import type { RemoteBridgeStatus } from '@k-3720/remote/bridge';

const mocks = vi.hoisted(() => ({
  startRemoteAccess: vi.fn(),
}));

vi.mock('#/remote-access', () => ({ startRemoteAccess: mocks.startRemoteAccess }));

function makeTui(): KimiTUI {
  const harness = {
    close: vi.fn(async () => {}),
    track: vi.fn(),
    setTelemetryContext: vi.fn(),
  };
  const input: KimiTUIStartupInput = {
    cliOptions: {
      session: undefined,
      continue: false,
      yolo: false,
      auto: false,
      plan: false,
      model: undefined,
      outputFormat: undefined,
      prompt: undefined,
      skillsDirs: [],
      agent: undefined,
      agentFiles: [],
    },
    tuiConfig: {
      theme: 'dark',
      disablePasteBurst: false,
      busyInputMode: 'steer',
      editorCommand: null,
      notifications: { enabled: true, condition: 'unfocused' },
      upgrade: { autoInstall: true },
      statusLine: { items: null, command: null },
    },
    version: '0.0.0-test',
    workDir: '/tmp/remote-lifecycle',
  };
  return new KimiTUI(harness as never, input);
}

describe('TUI remote lifecycle', () => {
  beforeEach(() => vi.clearAllMocks());

  it('projects bridge status and stops the same remote instance', async () => {
    const close = vi.fn(async () => {});
    const createPairingUri = vi.fn(() => 'k-3720://pair?runtime=1');
    let onStatus: ((status: RemoteBridgeStatus) => void) | undefined;
    mocks.startRemoteAccess.mockImplementationOnce(async (options) => {
      onStatus = options.onStatus;
      return { runtimeId: 'runtime-1', createPairingUri, close };
    });
    const tui = makeTui();

    const started = await tui.startRemoteAccess();
    onStatus?.('online');

    expect(started.started).toBe(true);
    expect(tui.state.appState.remoteStatus).toBe('online');
    expect((await tui.startRemoteAccess()).started).toBe(false);
    expect(createPairingUri).not.toHaveBeenCalled();
    await expect(tui.pairRemoteAccess()).resolves.toEqual({
      runtimeId: 'runtime-1',
      pairingUri: 'k-3720://pair?runtime=1',
    });
    expect(createPairingUri).toHaveBeenCalledOnce();
    expect(mocks.startRemoteAccess).toHaveBeenCalledOnce();
    expect(await tui.stopRemoteAccess()).toBe(true);
    expect(close).toHaveBeenCalledOnce();
    expect(tui.state.appState.remoteStatus).toBeNull();
  });

  it('closes remote access when the TUI exits', async () => {
    const close = vi.fn(async () => {});
    mocks.startRemoteAccess.mockResolvedValueOnce({
      runtimeId: 'runtime-1',
      createPairingUri: vi.fn(() => 'k-3720://pair?runtime=1'),
      close,
    });
    const tui = makeTui();
    vi.spyOn(tui.state.terminal, 'drainInput').mockResolvedValue();
    vi.spyOn(tui.state.ui, 'stop').mockImplementation(() => {});

    await tui.startRemoteAccess();
    await tui.stop();

    expect(close).toHaveBeenCalledOnce();
  });
});
