import { describe, expect, it, vi } from 'vitest';

import {
  applyBusyInputModeChoice,
  applyUpdatePreferenceChoice,
} from '#/tui/commands/config';
import { darkColors } from '#/tui/theme/colors';

const mocks = vi.hoisted(() => ({
  saveTuiConfig: vi.fn(),
}));

vi.mock('../../../src/tui/config', async () => {
  const actual = await vi.importActual<typeof import('../../../src/tui/config.js')>(
    '../../../src/tui/config.js',
  );
  return {
    ...actual,
    saveTuiConfig: mocks.saveTuiConfig,
  };
});

function makeHost(overrides: Record<string, unknown> = {}) {
  return {
    state: {
      appState: {
        theme: 'auto' as const,
        editorCommand: null,
        disablePasteBurst: false,
        busyInputMode: 'steer' as const,
        notifications: { enabled: true, condition: 'unfocused' as const },
        upgrade: { autoInstall: true },
        statusLine: { items: null, command: null },
        ...overrides,
      },
      theme: { palette: darkColors },
    },
    setAppState: vi.fn(),
    showStatus: vi.fn(),
    track: vi.fn(),
  };
}

describe('update preference commands', () => {
  it('saves automatic update preference changes to tui.toml', async () => {
    const host = makeHost();

    await applyUpdatePreferenceChoice(host, false);

    expect(mocks.saveTuiConfig).toHaveBeenCalledWith({
      theme: 'auto',
      editorCommand: null,
      disablePasteBurst: false,
      busyInputMode: 'steer',
      notifications: { enabled: true, condition: 'unfocused' },
      upgrade: { autoInstall: false },
      statusLine: { items: null, command: null },
    });
    expect(host.setAppState).toHaveBeenCalledWith({ upgrade: { autoInstall: false } });
    expect(host.track).toHaveBeenCalledWith('upgrade_preference_changed', { auto_install: false });
    expect(host.showStatus).toHaveBeenCalledWith('Automatic updates disabled.');
  });

  it('saves busy input mode changes to tui.toml', async () => {
    const host = makeHost();

    await applyBusyInputModeChoice(host, 'queue');

    expect(mocks.saveTuiConfig).toHaveBeenCalledWith(
      expect.objectContaining({ busyInputMode: 'queue' }),
    );
    expect(host.setAppState).toHaveBeenCalledWith({ busyInputMode: 'queue' });
    expect(host.track).toHaveBeenCalledWith('busy_input_mode_changed', { mode: 'queue' });
    expect(host.showStatus).toHaveBeenCalledWith(
      'Busy input: Enter queues; use Ctrl-S to steer immediately.',
    );
  });

  it('no-ops when busy input mode is unchanged', async () => {
    const host = makeHost({ busyInputMode: 'steer' });
    mocks.saveTuiConfig.mockClear();

    await applyBusyInputModeChoice(host, 'steer');

    expect(mocks.saveTuiConfig).not.toHaveBeenCalled();
    expect(host.setAppState).not.toHaveBeenCalled();
  });
});
