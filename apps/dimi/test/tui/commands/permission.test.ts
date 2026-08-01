import { describe, expect, it, vi } from 'vitest';
import type { ModelAlias } from '@dimi-agent/dimi-sdk';

import type { SlashCommandHost } from '#/tui/commands';
import {
  applyPermissionChoice,
  handleAutoCommand,
  handleEffortCommand,
  handlePermissionCommand,
  handleYoloCommand,
  performModelSwitch,
} from '#/tui/commands/config';

function makeHost(overrides: Record<string, unknown> = {}) {
  const session = {
    setPermission: vi.fn(async () => {}),
  };
  const harness = {
    setConfig: vi.fn(async () => ({})),
  };
  return {
    state: {
      appState: {
        permissionMode: 'manual' as const,
        ...(overrides['appState'] as Record<string, unknown> | undefined),
      },
    },
    session,
    harness,
    requireSession: () => session,
    setAppState: vi.fn(),
    showStatus: vi.fn(),
    showNotice: vi.fn(),
    showError: vi.fn(),
    mountEditorReplacement: vi.fn(),
    restoreEditor: vi.fn(),
    ...overrides,
  } as unknown as SlashCommandHost & {
    session: { setPermission: ReturnType<typeof vi.fn> };
    harness: { setConfig: ReturnType<typeof vi.fn> };
    setAppState: ReturnType<typeof vi.fn>;
    showStatus: ReturnType<typeof vi.fn>;
    showNotice: ReturnType<typeof vi.fn>;
    showError: ReturnType<typeof vi.fn>;
    mountEditorReplacement: ReturnType<typeof vi.fn>;
  };
}

describe('permission commands', () => {
  it('applies and persists the chosen mode as the default for new sessions', async () => {
    const host = makeHost();

    await handlePermissionCommand(host, 'yolo');

    expect(host.session.setPermission).toHaveBeenCalledWith('yolo');
    expect(host.setAppState).toHaveBeenCalledWith({ permissionMode: 'yolo' });
    expect(host.harness.setConfig).toHaveBeenCalledWith({ defaultPermissionMode: 'yolo' });
    expect(host.showNotice).toHaveBeenCalledWith(
      'Permission mode: yolo',
      expect.stringContaining('default for new sessions'),
    );
  });

  it('applies to the current session only when persistDefault is false', async () => {
    const host = makeHost();

    await applyPermissionChoice(host, 'auto', false);

    expect(host.session.setPermission).toHaveBeenCalledWith('auto');
    expect(host.setAppState).toHaveBeenCalledWith({ permissionMode: 'auto' });
    expect(host.harness.setConfig).not.toHaveBeenCalled();
    expect(host.showNotice).toHaveBeenCalledWith(
      'Permission mode: auto',
      expect.stringContaining('this session only'),
    );
  });

  it('opens the picker when invoked without an argument', async () => {
    const host = makeHost();

    await handlePermissionCommand(host, '');

    expect(host.mountEditorReplacement).toHaveBeenCalledTimes(1);
    expect(host.session.setPermission).not.toHaveBeenCalled();
    expect(host.harness.setConfig).not.toHaveBeenCalled();
  });

  it('rejects an unknown mode argument', async () => {
    const host = makeHost();

    await handlePermissionCommand(host, 'maybe');

    expect(host.showError).toHaveBeenCalledWith(
      'Unknown permission mode: maybe. Use manual, yolo, or auto.',
    );
    expect(host.session.setPermission).not.toHaveBeenCalled();
  });

  it('no-ops when the mode is unchanged', async () => {
    const host = makeHost({ appState: { permissionMode: 'yolo' } });

    await applyPermissionChoice(host, 'yolo', true);

    expect(host.showStatus).toHaveBeenCalledWith('Permission mode unchanged: yolo.');
    expect(host.session.setPermission).not.toHaveBeenCalled();
    expect(host.harness.setConfig).not.toHaveBeenCalled();
    expect(host.setAppState).not.toHaveBeenCalled();
  });

  it('reports a session failure without persisting', async () => {
    const session = { setPermission: vi.fn(async () => { throw new Error('nope'); }) };
    const host = makeHost({ session, requireSession: () => session });

    await applyPermissionChoice(host, 'yolo', true);

    expect(host.showError).toHaveBeenCalledWith('Failed to set permission mode: nope');
    expect(host.harness.setConfig).not.toHaveBeenCalled();
    expect(host.setAppState).not.toHaveBeenCalled();
  });

  it('keeps the session change but reports when saving the default fails', async () => {
    const harness = { setConfig: vi.fn(async () => { throw new Error('disk full'); }) };
    const host = makeHost({ harness });

    await applyPermissionChoice(host, 'auto', true);

    expect(host.setAppState).toHaveBeenCalledWith({ permissionMode: 'auto' });
    expect(host.showError).toHaveBeenCalledWith(
      'Permission mode: auto for this session, but failed to save default: disk full',
    );
  });

  it('persists yolo mode when /yolo on is run', async () => {
    const host = makeHost();

    await handleYoloCommand(host, 'on');

    expect(host.session.setPermission).toHaveBeenCalledWith('yolo');
    expect(host.setAppState).toHaveBeenCalledWith({ permissionMode: 'yolo' });
    expect(host.harness.setConfig).toHaveBeenCalledWith({ defaultPermissionMode: 'yolo' });
    expect(host.showNotice).toHaveBeenCalledWith(
      'YOLO mode: ON',
      expect.stringContaining('Saved as the default for new sessions'),
    );
  });

  it('persists manual mode when /yolo off is run', async () => {
    const host = makeHost({ appState: { permissionMode: 'yolo' } });

    await handleYoloCommand(host, 'off');

    expect(host.session.setPermission).toHaveBeenCalledWith('manual');
    expect(host.harness.setConfig).toHaveBeenCalledWith({ defaultPermissionMode: 'manual' });
    expect(host.showNotice).toHaveBeenCalledWith('YOLO mode: OFF');
  });

  it('persists the toggled mode when /yolo is run without arguments', async () => {
    const host = makeHost();

    await handleYoloCommand(host, '');

    expect(host.session.setPermission).toHaveBeenCalledWith('yolo');
    expect(host.harness.setConfig).toHaveBeenCalledWith({ defaultPermissionMode: 'yolo' });
  });

  it('persists auto mode when /auto on is run', async () => {
    const host = makeHost();

    await handleAutoCommand(host, 'on');

    expect(host.session.setPermission).toHaveBeenCalledWith('auto');
    expect(host.harness.setConfig).toHaveBeenCalledWith({ defaultPermissionMode: 'auto' });
    expect(host.showNotice).toHaveBeenCalledWith(
      'Auto mode: ON',
      expect.stringContaining('Saved as the default for new sessions'),
    );
  });

  it('persists manual mode when /auto off is run', async () => {
    const host = makeHost({ appState: { permissionMode: 'auto' } });

    await handleAutoCommand(host, 'off');

    expect(host.session.setPermission).toHaveBeenCalledWith('manual');
    expect(host.harness.setConfig).toHaveBeenCalledWith({ defaultPermissionMode: 'manual' });
    expect(host.showNotice).toHaveBeenCalledWith('Auto mode: OFF');
  });

  it('does not re-persist when the mode is already active', async () => {
    const host = makeHost({ appState: { permissionMode: 'yolo' } });

    await handleYoloCommand(host, 'on');

    expect(host.showNotice).toHaveBeenCalledWith('YOLO mode is already on');
    expect(host.harness.setConfig).not.toHaveBeenCalled();
  });
});

function model(provider: string, id: string): ModelAlias {
  return {
    provider,
    model: id,
    maxContextSize: 200_000,
    displayName: id,
    supportEfforts: ['low', 'medium', 'high'],
    capabilities: ['thinking'],
  } as unknown as ModelAlias;
}

/** Host rig for model/effort switching with a live session. */
function makeModelHost(options: {
  currentAlias?: string;
  currentEffort?: string;
  config?: Record<string, unknown>;
  availableModels?: Record<string, ModelAlias>;
} = {}) {
  let alias = options.currentAlias ?? 'test/k2';
  let effort = options.currentEffort ?? 'medium';
  const session = {
    setPermission: vi.fn(async () => {}),
    setModel: vi.fn(async (next: string) => {
      alias = next;
    }),
    setThinking: vi.fn(async (next: string) => {
      effort = next;
    }),
    getStatus: vi.fn(async () => ({ model: alias, thinkingEffort: effort })),
  };
  const harness = {
    getConfig: vi.fn(async () => options.config ?? {}),
    setConfig: vi.fn(async () => ({})),
  };
  const host = {
    state: {
      appState: {
        model: options.currentAlias ?? 'test/k2',
        thinkingEffort: options.currentEffort ?? 'medium',
        streamingPhase: 'idle',
        availableModels: options.availableModels ?? { 'test/k2': model('test', 'k2') },
      },
    },
    session,
    harness,
    authFlow: {},
    requireSession: () => session,
    setAppState: vi.fn((patch: Record<string, unknown>) => {
      Object.assign(host.state.appState, patch);
    }),
    showStatus: vi.fn(),
    showNotice: vi.fn(),
    showError: vi.fn(),
    track: vi.fn(),
    mountEditorReplacement: vi.fn(),
    restoreEditor: vi.fn(),
  };
  return host as unknown as SlashCommandHost & {
    session: {
      setPermission: ReturnType<typeof vi.fn>;
      setModel: ReturnType<typeof vi.fn>;
      setThinking: ReturnType<typeof vi.fn>;
      getStatus: ReturnType<typeof vi.fn>;
    };
    harness: { getConfig: ReturnType<typeof vi.fn>; setConfig: ReturnType<typeof vi.fn> };
    setAppState: ReturnType<typeof vi.fn>;
    showStatus: ReturnType<typeof vi.fn>;
    showNotice: ReturnType<typeof vi.fn>;
    showError: ReturnType<typeof vi.fn>;
  };
}

describe('per-model effort memory ([model_efforts])', () => {
  it('persists the effort under the (provider, model) key when /effort is run', async () => {
    const host = makeModelHost({ currentEffort: 'medium' });

    await handleEffortCommand(host, 'high');

    expect(host.session.setThinking).toHaveBeenCalledWith('high');
    expect(host.harness.setConfig).toHaveBeenCalledWith(
      expect.objectContaining({
        modelEfforts: { 'test/k2': 'high' },
        thinking: { enabled: true, effort: 'high' },
      }),
    );
  });

  it('restores the target model remembered effort when switching without changing it', async () => {
    const host = makeModelHost({
      config: { modelEfforts: { 'test/k3': 'high' } },
      availableModels: { 'test/k2': model('test', 'k2'), 'test/k3': model('test', 'k3') },
    });

    await performModelSwitch(host, 'test/k3', 'medium', true);

    // The user did not touch the effort (picker still showed 'medium'), so the
    // remembered 'high' for test/k3 wins.
    expect(host.session.setThinking).toHaveBeenCalledWith('high');
    expect(host.state.appState.thinkingEffort).toBe('high');
    expect(host.harness.setConfig).toHaveBeenCalledWith(
      expect.objectContaining({ modelEfforts: { 'test/k3': 'high' } }),
    );
  });

  it('keeps the explicitly chosen effort over the remembered one', async () => {
    const host = makeModelHost({
      config: { modelEfforts: { 'test/k3': 'high' } },
      availableModels: { 'test/k2': model('test', 'k2'), 'test/k3': model('test', 'k3') },
    });

    await performModelSwitch(host, 'test/k3', 'low', true);

    expect(host.session.setThinking).toHaveBeenCalledTimes(1);
    expect(host.session.setThinking).toHaveBeenCalledWith('low');
    expect(host.harness.setConfig).toHaveBeenCalledWith(
      expect.objectContaining({ modelEfforts: { 'test/k3': 'low' } }),
    );
  });

  it('does not re-write config when the remembered effort already matches', async () => {
    const host = makeModelHost({
      currentEffort: 'high',
      config: {
        defaultProvider: 'test',
        defaultModel: 'k2',
        thinking: { enabled: true, effort: 'high' },
        modelEfforts: { 'test/k2': 'high' },
      },
    });

    await handleEffortCommand(host, 'high');

    expect(host.session.setThinking).not.toHaveBeenCalled();
    expect(host.harness.setConfig).not.toHaveBeenCalled();
  });
});
