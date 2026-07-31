import { beforeEach, describe, expect, it, vi } from 'vitest';

import { dispatchInput, type SlashCommandHost } from '#/tui/commands/dispatch';
import { handleRemoteCommand } from '#/tui/commands/remote';

const mocks = vi.hoisted(() => ({
  toString: vi.fn(async () => '<qr>'),
}));

vi.mock('qrcode', () => ({ default: { toString: mocks.toString } }));

function makeHost(): SlashCommandHost & {
  showError: ReturnType<typeof vi.fn>;
  showNotice: ReturnType<typeof vi.fn>;
  startRemoteAccess: ReturnType<typeof vi.fn>;
  pairRemoteAccess: ReturnType<typeof vi.fn>;
  stopRemoteAccess: ReturnType<typeof vi.fn>;
} {
  return {
    state: { appState: { streamingPhase: 'idle', isCompacting: false } },
    skillCommandMap: new Map(),
    pluginCommandMap: new Map(),
    track: vi.fn(),
    showError: vi.fn(),
    showNotice: vi.fn(),
    startRemoteAccess: vi.fn(async () => ({
      runtimeId: 'runtime-1',
      started: true,
    })),
    pairRemoteAccess: vi.fn(async () => ({
      runtimeId: 'runtime-1',
      pairingUri: 'k-3720://pair?runtime=1',
    })),
    stopRemoteAccess: vi.fn(async () => true),
  } as unknown as SlashCommandHost & {
    showError: ReturnType<typeof vi.fn>;
    showNotice: ReturnType<typeof vi.fn>;
    startRemoteAccess: ReturnType<typeof vi.fn>;
    pairRemoteAccess: ReturnType<typeof vi.fn>;
    stopRemoteAccess: ReturnType<typeof vi.fn>;
  };
}

describe('remote slash command', () => {
  beforeEach(() => vi.clearAllMocks());

  it('starts remote access without creating a pairing code', async () => {
    const host = makeHost();

    dispatchInput(host, '/remote start');
    await vi.waitFor(() => {
      expect(host.showNotice).toHaveBeenCalledOnce();
    });

    expect(host.startRemoteAccess).toHaveBeenCalledOnce();
    expect(host.pairRemoteAccess).not.toHaveBeenCalled();
    expect(mocks.toString).not.toHaveBeenCalled();
    expect(host.showNotice).toHaveBeenCalledWith(
      'Remote access started',
      expect.stringContaining('Paired devices connect automatically'),
    );
  });

  it('creates pairing details only for the pair action', async () => {
    const host = makeHost();

    await handleRemoteCommand(host, 'pair');

    expect(host.pairRemoteAccess).toHaveBeenCalledOnce();
    expect(mocks.toString).toHaveBeenCalledWith('k-3720://pair?runtime=1', {
      type: 'terminal',
      small: true,
    });
    expect(host.showNotice).toHaveBeenCalledWith(
      'Pair a mobile device',
      expect.stringContaining('k-3720://pair?runtime=1'),
    );
  });

  it('stops remote access', async () => {
    const host = makeHost();

    await handleRemoteCommand(host, 'stop');

    expect(host.stopRemoteAccess).toHaveBeenCalledOnce();
    expect(host.showNotice).toHaveBeenCalledWith('Remote access stopped');
  });

  it('reports idempotent start and stop states', async () => {
    const host = makeHost();
    host.startRemoteAccess.mockResolvedValueOnce({
      runtimeId: 'runtime-1',
      started: false,
    });
    host.stopRemoteAccess.mockResolvedValueOnce(false);

    await handleRemoteCommand(host, 'start');
    await handleRemoteCommand(host, 'stop');

    expect(host.showNotice).toHaveBeenNthCalledWith(
      1,
      'Remote access is already running',
      expect.stringContaining('Paired devices connect automatically'),
    );
    expect(host.showNotice).toHaveBeenNthCalledWith(2, 'Remote access is already stopped');
  });

  it('rejects missing or unknown actions', async () => {
    const host = makeHost();

    await handleRemoteCommand(host, '');
    await handleRemoteCommand(host, 'restart');

    expect(host.showError).toHaveBeenCalledTimes(2);
    expect(host.showError).toHaveBeenLastCalledWith('Usage: /remote <start|pair|stop>');
    expect(host.startRemoteAccess).not.toHaveBeenCalled();
    expect(host.pairRemoteAccess).not.toHaveBeenCalled();
    expect(host.stopRemoteAccess).not.toHaveBeenCalled();
  });
});
