import {
  type ExperimentalFeatureState,
  type DimiConfig,
  type ModelAlias,
  type PermissionMode,
  type Session,
  type ThinkingEffort,
} from '@dimi-agent/dimi-sdk';

import { BusyInputModeSelectorComponent } from '../components/dialogs/busy-input-mode-selector';
import { EditorSelectorComponent } from '../components/dialogs/editor-selector';
import { EffortSelectorComponent } from '../components/dialogs/effort-selector';
import {
  ExperimentsSelectorComponent,
  type ExperimentalFeatureDraftChange,
} from '../components/dialogs/experiments-selector';
import { modelDisplayName, segmentsFor } from '../components/dialogs/model-selector';
import { TabbedModelSelectorComponent } from '../components/dialogs/tabbed-model-selector';
import { PermissionSelectorComponent } from '../components/dialogs/permission-selector';
import {
  SettingsSelectorComponent,
  type SettingsSelection,
} from '../components/dialogs/settings-selector';
import { ThemeSelectorComponent } from '../components/dialogs/theme-selector';
import { UpdatePreferenceSelectorComponent } from '../components/dialogs/update-preference-selector';
import { DEFAULT_TUI_CONFIG, saveTuiConfig, type BusyInputMode, type TuiConfig } from '../config';
import type { ThemeName } from '#/tui/theme';
import { currentTheme, isBuiltInTheme, lightColors, loadCustomThemeMerged } from '#/tui/theme';
import { NO_ACTIVE_SESSION_MESSAGE } from '../constant/dimi-tui';
import { formatErrorMessage } from '../utils/event-payload';
import {
  modelEffortKey,
  rememberedEffortFromConfig,
  thinkingEffortToConfig,
} from '../utils/thinking-config';
import { showUsage } from './info';
import { setExperimentalFeatures } from './experimental-flags';
import type { SlashCommandHost } from './dispatch';

// ---------------------------------------------------------------------------
// Plan / Config commands
// ---------------------------------------------------------------------------

const MODEL_PICKER_REFRESH_TIMEOUT_MS = 2_000;

const MODEL_SWITCH_CACHE_WARNING =
  'Note: Switching models invalidates the existing prompt cache. Use /new to avoid extra token costs.';
const EFFORT_SWITCH_CACHE_WARNING =
  'Note: Switching effort invalidates the existing prompt cache. Use /new to avoid extra token costs.';

/** True once the conversation has at least one user message: a switch from
 * then on resends the accumulated context, losing the cache. Shell-command
 * echoes are also 'user' transcript entries but carry an empty `bullet`, so
 * they're excluded. */
function hasConversationHistory(host: SlashCommandHost): boolean {
  return host.state.transcriptEntries.some((entry) => entry.kind === 'user' && entry.bullet !== '');
}

function currentTuiConfig(host: SlashCommandHost): TuiConfig {
  return {
    theme: host.state.appState.theme,
    editorCommand: host.state.appState.editorCommand,
    disablePasteBurst:
      host.state.appState.disablePasteBurst ?? DEFAULT_TUI_CONFIG.disablePasteBurst,
    busyInputMode: host.state.appState.busyInputMode ?? DEFAULT_TUI_CONFIG.busyInputMode,
    notifications: host.state.appState.notifications,
    upgrade: host.state.appState.upgrade,
    statusLine: host.state.appState.statusLine ?? DEFAULT_TUI_CONFIG.statusLine,
  };
}

export function effectiveModelForHost(host: SlashCommandHost, model: ModelAlias): ModelAlias {
  void host;
  return model;
}

export async function handlePlanCommand(host: SlashCommandHost, args: string): Promise<void> {
  const session = host.session;
  if (session === undefined) {
    host.showError(NO_ACTIVE_SESSION_MESSAGE);
    return;
  }

  const subcmd = args.trim().toLowerCase();
  if (subcmd === 'clear') {
    await session.clearPlan();
    host.showNotice('Plan cleared');
    return;
  }

  let enabled: boolean;
  if (subcmd.length === 0) enabled = !host.state.appState.planMode;
  else if (subcmd === 'on') enabled = true;
  else if (subcmd === 'off') enabled = false;
  else {
    host.showError(`Unknown plan subcommand: ${subcmd}`);
    return;
  }

  await applyPlanMode(host, session, enabled);
}

async function applyPlanMode(
  host: SlashCommandHost,
  session: Session,
  enabled: boolean,
): Promise<void> {
  try {
    await session.setPlanMode(enabled);
    host.setAppState({ planMode: enabled });
    if (enabled) {
      const plan = await session.getPlan().catch(() => null);
      host.showNotice(
        'Plan mode: ON',
        plan?.path !== undefined ? `Plan will be created here: ${plan.path}` : undefined,
      );
      return;
    }
    host.showNotice('Plan mode: OFF');
  } catch (error) {
    const msg = formatErrorMessage(error);
    host.showError(`Failed to set plan mode: ${msg}`);
  }
}

export async function handleYoloCommand(host: SlashCommandHost, args: string): Promise<void> {
  const session = host.session;
  if (session === undefined) {
    host.showError(NO_ACTIVE_SESSION_MESSAGE);
    return;
  }

  const subcmd = args.trim().toLowerCase();
  const currentMode = host.state.appState.permissionMode;

  if (subcmd === 'on') {
    if (currentMode === 'yolo') {
      host.showNotice('YOLO mode is already on');
      return;
    }
    if (!(await applyPermissionModeWithDefault(host, 'yolo'))) return;
    host.showNotice(
      'YOLO mode: ON',
      'Tool actions auto-approved; the agent may still ask you questions. Saved as the default for new sessions.',
    );
    return;
  }

  if (subcmd === 'off') {
    if (currentMode !== 'yolo') {
      host.showNotice('YOLO mode is already off');
      return;
    }
    if (!(await applyPermissionModeWithDefault(host, 'manual'))) return;
    host.showNotice('YOLO mode: OFF');
    return;
  }

  // toggle
  if (currentMode === 'yolo') {
    if (!(await applyPermissionModeWithDefault(host, 'manual'))) return;
    host.showNotice('YOLO mode: OFF');
  } else {
    if (!(await applyPermissionModeWithDefault(host, 'yolo'))) return;
    host.showNotice(
      'YOLO mode: ON',
      'Tool actions auto-approved; the agent may still ask you questions. Saved as the default for new sessions.',
    );
  }
}

export async function handleAutoCommand(host: SlashCommandHost, args: string): Promise<void> {
  const session = host.session;
  if (session === undefined) {
    host.showError(NO_ACTIVE_SESSION_MESSAGE);
    return;
  }

  const subcmd = args.trim().toLowerCase();
  const currentMode = host.state.appState.permissionMode;

  if (subcmd === 'on') {
    if (currentMode === 'auto') {
      host.showNotice('Auto mode is already on');
      return;
    }
    if (!(await applyPermissionModeWithDefault(host, 'auto'))) return;
    host.showNotice(
      'Auto mode: ON',
      'All actions auto-approved; the agent will not ask you questions. Saved as the default for new sessions.',
    );
    return;
  }

  if (subcmd === 'off') {
    if (currentMode !== 'auto') {
      host.showNotice('Auto mode is already off');
      return;
    }
    if (!(await applyPermissionModeWithDefault(host, 'manual'))) return;
    host.showNotice('Auto mode: OFF');
    return;
  }

  // toggle
  if (currentMode === 'auto') {
    if (!(await applyPermissionModeWithDefault(host, 'manual'))) return;
    host.showNotice('Auto mode: OFF');
  } else {
    if (!(await applyPermissionModeWithDefault(host, 'auto'))) return;
    host.showNotice(
      'Auto mode: ON',
      'All actions auto-approved; the agent will not ask you questions. Saved as the default for new sessions.',
    );
  }
}

/**
 * Apply a permission mode to the current session and persist it as the
 * default for new sessions (`default_permission_mode`). On failure shows the
 * error and returns false; an already-applied session change is not rolled
 * back (matching the model-switch behavior).
 */
async function applyPermissionModeWithDefault(
  host: SlashCommandHost,
  mode: PermissionMode,
): Promise<boolean> {
  try {
    await host.requireSession().setPermission(mode);
  } catch (error) {
    host.showError(`Failed to set permission mode: ${formatErrorMessage(error)}`);
    return false;
  }
  host.setAppState({ permissionMode: mode });
  try {
    await host.harness.setConfig({ defaultPermissionMode: mode });
  } catch (error) {
    host.showError(
      `Permission mode: ${mode} for this session, but failed to save default: ${formatErrorMessage(error)}`,
    );
    return false;
  }
  return true;
}

export async function handleCompactCommand(host: SlashCommandHost, args: string): Promise<void> {
  const session = host.session;
  if (session === undefined) {
    host.showError(NO_ACTIVE_SESSION_MESSAGE);
    return;
  }
  const customInstruction = args.trim() || undefined;
  await session.compact({ instruction: customInstruction });
}

export async function handleEditorCommand(host: SlashCommandHost, args: string): Promise<void> {
  const command = args.trim();
  if (command.length === 0) {
    showEditorPicker(host);
    return;
  }
  await applyEditorChoice(host, command);
}

export async function handleThemeCommand(host: SlashCommandHost, args: string): Promise<void> {
  const theme = args.trim();
  if (theme.length === 0) {
    showThemePicker(host);
    return;
  }
  if (!isBuiltInTheme(theme)) {
    const custom = await loadCustomThemeMerged(theme);
    if (custom === null) {
      host.showError(`Unknown theme: ${theme}`);
      return;
    }
  }
  await applyThemeChoice(host, theme);
}

export async function handleModelCommand(host: SlashCommandHost, args: string): Promise<void> {
  const reference = args.trim();
  await refreshModelsForPicker(host);
  const diagnostic = await host.harness.auth.providerDefinitionDiagnostic();
  if (diagnostic !== undefined) host.showError(diagnostic);
  if (reference.length === 0) {
    showModelPicker(host);
    return;
  }
  if (host.state.appState.availableModels[reference] === undefined) {
    host.showError(`Unknown model: ${reference}`);
    return;
  }
  showModelPicker(host, reference);
}

export async function handleSecondaryModelCommand(
  host: SlashCommandHost,
  args: string,
): Promise<void> {
  const reference = args.trim();
  await refreshModelsForPicker(host);
  const models = pickerModelsForHost(host);
  if (Object.keys(models).length === 0) {
    host.showNotice('No models configured', 'Run /login to connect a provider.');
    return;
  }
  if (reference.length > 0 && models[reference] === undefined) {
    host.showError(`Unknown model: ${reference}`);
    return;
  }
  const secondary = (await host.harness.getConfig()).secondaryModel;
  const currentValue = configuredModelReference(models, secondary?.provider, secondary?.model);
  showSecondaryModelPicker(host, models, currentValue, secondary?.defaultEffort, reference);
}

export async function handleEffortCommand(host: SlashCommandHost, args: string): Promise<void> {
  const alias = host.state.appState.model;
  const model = host.state.appState.availableModels[alias];
  if (model === undefined) {
    host.showError('No model selected. Run /model to select one first.');
    return;
  }
  const effective = effectiveModelForHost(host, model);
  const segments = segmentsFor(effective);
  const arg = args.trim().toLowerCase();
  if (arg.length === 0) {
    showEffortPicker(host, effective, segments);
    return;
  }
  if (!segments.includes(arg)) {
    host.showError(
      `Unsupported thinking effort "${arg}" for ${alias}. Available: ${segments.join(', ')}`,
    );
    return;
  }
  await performModelSwitch(host, alias, arg, true);
}

function showEffortPicker(
  host: SlashCommandHost,
  model: ModelAlias,
  segments: readonly string[],
): void {
  const liveEffort = host.state.appState.thinkingEffort;
  const currentValue = segments.includes(liveEffort) ? liveEffort : segments[0] ?? 'off';
  const alias = host.state.appState.model;
  host.mountEditorReplacement(
    new EffortSelectorComponent({
      efforts: segments,
      currentValue,
      warning: hasConversationHistory(host) ? EFFORT_SWITCH_CACHE_WARNING : undefined,
      onSelect: (effort) => {
        host.restoreEditor();
        void performModelSwitch(host, alias, effort, true);
      },
      onSessionOnlySelect: (effort) => {
        host.restoreEditor();
        void performModelSwitch(host, alias, effort, false);
      },
      onCancel: () => {
        host.restoreEditor();
      },
    }),
  );
}

// ---------------------------------------------------------------------------
// Pickers & config apply
// ---------------------------------------------------------------------------

function showEditorPicker(host: SlashCommandHost): void {
  const currentValue = host.state.appState.editorCommand ?? '';
  host.mountEditorReplacement(
    new EditorSelectorComponent({
      currentValue,
      onSelect: (value) => {
        host.restoreEditor();
        void applyEditorChoice(host, value);
      },
      onCancel: () => {
        host.restoreEditor();
      },
    }),
  );
}

async function refreshModelsForPicker(host: SlashCommandHost): Promise<void> {
  try {
    const result = await withTimeout(
      host.authFlow.refreshProviderModels(),
      MODEL_PICKER_REFRESH_TIMEOUT_MS,
    );
    if (result === undefined) return;
    for (const f of result.failed) {
      host.showStatus(`Skipped refreshing ${f.provider}: ${f.reason}`, 'warning');
    }
  } catch (error) {
    host.showStatus(`Skipped refreshing models: ${formatErrorMessage(error)}`, 'warning');
  }
}

async function withTimeout<T>(promise: Promise<T>, timeoutMs: number): Promise<T | undefined> {
  let timeout: ReturnType<typeof setTimeout> | undefined;
  try {
    return await Promise.race([
      promise,
      new Promise<undefined>((resolve) => {
        timeout = setTimeout(() => {
          resolve(undefined);
        }, timeoutMs);
      }),
    ]);
  } finally {
    if (timeout !== undefined) clearTimeout(timeout);
  }
}

async function applyEditorChoice(host: SlashCommandHost, value: string): Promise<void> {
  const previous = host.state.appState.editorCommand ?? '';
  if (value === previous && value.length > 0) {
    host.showStatus(`Editor unchanged: ${value.length > 0 ? value : 'auto-detect'}`);
    return;
  }

  const editorCommand = value.length > 0 ? value : null;
  try {
    await saveTuiConfig({
      ...currentTuiConfig(host),
      editorCommand,
    });
  } catch (error) {
    host.showStatus(`Failed to save editor: ${formatErrorMessage(error)}`, 'error');
    return;
  }

  host.setAppState({ editorCommand });
  host.showStatus(
    value.length > 0
      ? `Editor set to "${value}".`
      : 'Editor set to auto-detect ($VISUAL / $EDITOR).',
  );
}

/**
 * The models a picker may offer: the authenticated runtime catalog with
 * host-effective provider resolution applied.
 */
function pickerModelsForHost(host: SlashCommandHost): Record<string, ModelAlias> {
  return Object.fromEntries(
    Object.entries(host.state.appState.availableModels).map(([alias, model]) => [
      alias,
      effectiveModelForHost(host, model),
    ]),
  );
}

function configuredModelReference(
  models: Record<string, ModelAlias>,
  provider: string | undefined,
  model: string | undefined,
): string {
  if (model === undefined) return '';
  if (provider !== undefined) return `${provider}/${model}`;
  const matches = Object.entries(models)
    .filter(([, entry]) => entry.model === model)
    .map(([reference]) => reference);
  return matches.length === 1 ? matches[0]! : model;
}

export function showModelPicker(
  host: SlashCommandHost,
  selectedValue: string = host.state.appState.model,
): void {
  const models = pickerModelsForHost(host);
  const entries = Object.entries(models);
  if (entries.length === 0) {
    host.showNotice('No models configured', 'Run /login to connect a provider.');
    return;
  }
  host.mountEditorReplacement(
    new TabbedModelSelectorComponent({
      models,
      currentValue: host.state.appState.model,
      selectedValue,
      currentThinkingEffort: host.state.appState.thinkingEffort,
      warning: hasConversationHistory(host) ? MODEL_SWITCH_CACHE_WARNING : undefined,
      onSelect: ({ alias, thinking }) => {
        host.restoreEditor();
        void performModelSwitch(host, alias, thinking, true);
      },
      onSessionOnlySelect: ({ alias, thinking }) => {
        host.restoreEditor();
        void performModelSwitch(host, alias, thinking, false);
      },
      onCancel: () => {
        host.restoreEditor();
      },
    }),
  );
}

/**
 * The effort the user last chose for a model, from `[model_efforts]` in
 * config.toml (`"provider/model" -> effort`). Returns `undefined` when the
 * model is unknown or has no recorded effort.
 */
async function rememberedEffortForModel(
  host: SlashCommandHost,
  model: ModelAlias | undefined,
): Promise<ThinkingEffort | undefined> {
  if (model === undefined) return undefined;
  const config = await host.harness.getConfig();
  if (config === undefined) return undefined;
  return rememberedEffortFromConfig(config, model);
}

/**
 * Apply a model / thinking-effort switch. Safe while a turn is streaming or
 * the context is compacting: the engine snapshots model, effort, and system
 * prompt per turn, so the change takes effect from the next turn and never
 * races an in-flight request. The registry marks both `/model` and `/effort`
 * as `always`; this function must not re-gate them on busy state.
 */
export async function performModelSwitch(
  host: SlashCommandHost,
  alias: string,
  effort: ThinkingEffort,
  persist: boolean,
): Promise<void> {
  const prevModel = host.state.appState.model;
  const prevEffort = host.state.appState.thinkingEffort;
  const modelChanged = alias !== prevModel;
  const effortChanged = effort !== prevEffort;
  const runtimeChanged = modelChanged || effortChanged;
  let effectiveAlias = alias;
  let effectiveEffort = effort;

  const session = host.session;
  try {
    if (session === undefined && runtimeChanged) {
      await host.authFlow.activateModelAfterLogin(alias, effort);
    } else if (session !== undefined) {
      if (alias !== prevModel) {
        await session.setModel(alias);
      }
      if (effort !== prevEffort) {
        await session.setThinking(effort);
      }
      // Switching models without an explicit effort change restores the
      // target model's remembered thinking level ([model_efforts]) — each
      // model keeps the effort the user last chose for it.
      if (modelChanged && !effortChanged) {
        const remembered = await rememberedEffortForModel(
          host,
          host.state.appState.availableModels[alias],
        );
        if (remembered !== undefined && remembered !== prevEffort) {
          await session.setThinking(remembered);
        }
      }
      const status = await session.getStatus();
      effectiveAlias = status.model ?? alias;
      effectiveEffort = status.thinkingEffort;
    }
  } catch (error) {
    const msg = formatErrorMessage(error);
    host.showError(`Failed to switch model: ${msg}`);
    return;
  }

  if (session === undefined) {
    effectiveAlias = host.state.appState.model;
    effectiveEffort = host.state.appState.thinkingEffort;
  }
  const effectiveModelChanged = effectiveAlias !== prevModel;
  const effectiveEffortChanged = effectiveEffort !== prevEffort;
  const displayName = modelDisplayName(
    effectiveAlias,
    host.state.appState.availableModels[effectiveAlias],
  );
  host.setAppState({ model: effectiveAlias, thinkingEffort: effectiveEffort });
  if (session === undefined && runtimeChanged) {
    if (effectiveModelChanged) {
      host.track('model_switch', { model: effectiveAlias });
    }
    if (effectiveEffortChanged) {
      host.track('thinking_toggle', {
        enabled: effectiveEffort !== 'off',
        effort: effectiveEffort,
        from: prevEffort,
      });
    }
  }

  let persisted = false;
  if (persist) {
    try {
      persisted = await persistModelSelection(
        host,
        effectiveAlias,
        effectiveEffort,
        effectiveEffortChanged,
      );
    } catch (error) {
      const msg = formatErrorMessage(error);
      host.showError(`Switched to ${displayName}, but failed to save default: ${msg}`);
      return;
    }
  }

  let status: string;
  if (effectiveModelChanged) {
    status = persist
      ? `Switched to ${displayName} with thinking ${effectiveEffort}.`
      : `Switched to ${displayName} with thinking ${effectiveEffort} for this session only.`;
  } else if (effectiveEffortChanged) {
    status = persist
      ? `Thinking set to ${effectiveEffort}.`
      : `Thinking set to ${effectiveEffort} for this session only.`;
  } else if (persist && persisted) {
    status = `Saved ${displayName} with thinking ${effectiveEffort} as default.`;
  } else {
    status = `Already using ${displayName} with thinking ${effectiveEffort}.`;
  }
  host.showStatus(status, 'success');
}

async function persistModelSelection(
  host: SlashCommandHost,
  alias: string,
  effort: ThinkingEffort,
  effortChanged: boolean,
): Promise<boolean> {
  const config = await host.harness.getConfig({ reload: true });
  const model = host.state.appState.availableModels[alias];
  const full = thinkingEffortToConfig(effort);
  // Re-confirming the effort shown when the picker opened is not an explicit
  // choice — persist the model but leave the stored effort preference alone.
  const patch = effortChanged ? full : { enabled: full.enabled };
  const rememberedKey =
    model === undefined ? undefined : modelEffortKey(model.provider, model.model);
  const modelEfforts = config['modelEfforts'] as Record<string, string> | undefined;
  if (
    config.defaultProvider === model?.provider &&
    config.defaultModel === model?.model &&
    config.thinking?.enabled === patch.enabled &&
    (!effortChanged || config.thinking?.effort === patch.effort) &&
    // Only an explicit effort change records/checks the per-model memory;
    // re-confirming the current value is not an explicit choice.
    (!effortChanged || rememberedKey === undefined || modelEfforts?.[rememberedKey] === effort)
  ) {
    return false;
  }
  await host.harness.setConfig({
    defaultProvider: model?.provider,
    defaultModel: model?.model,
    thinking: patch,
    // Remember the effort per (provider, model) so switching back to this
    // model restores the level the user chose for it.
    ...(effortChanged && rememberedKey !== undefined
      ? { modelEfforts: { [rememberedKey]: effort } }
      : {}),
  });
  return true;
}

// ---------------------------------------------------------------------------
// Secondary model (`/secondary_model`)
// ---------------------------------------------------------------------------

function showSecondaryModelPicker(
  host: SlashCommandHost,
  models: Record<string, ModelAlias>,
  currentValue: string,
  currentEffort: string | undefined,
  selectedValue?: string,
): void {
  host.mountEditorReplacement(
    new TabbedModelSelectorComponent({
      models,
      currentValue,
      selectedValue,
      currentThinkingEffort: currentEffort ?? 'off',
      title: ' Select a secondary model (subagents)',
      onSelect: ({ alias, thinking }) => {
        host.restoreEditor();
        void performSecondaryModelSwitch(host, alias, thinking);
      },
      onCancel: () => {
        host.restoreEditor();
      },
    }),
  );
}

/**
 * Persist-first, then live-apply: the synthesized derived entry only exists in
 * the core config after a reload. No session-only variant — a session-local
 * recipe with patch fields would bind a derived alias the core config cannot
 * resolve.
 */
async function performSecondaryModelSwitch(
  host: SlashCommandHost,
  alias: string,
  effort: ThinkingEffort,
): Promise<void> {
  const displayName = modelDisplayName(alias, host.state.appState.availableModels[alias]);
  const selected = host.state.appState.availableModels[alias];
  let updatedConfig: DimiConfig;
  try {
    updatedConfig = await host.harness.setConfig({
      secondaryModel: {
        provider: selected?.provider,
        model: selected?.model ?? alias,
        defaultEffort: effort,
      },
    });
  } catch (error) {
    host.showError(`Failed to save secondary model: ${formatErrorMessage(error)}`);
    return;
  }
  if (host.session !== undefined) {
    try {
      await host.session.applyPersistedSecondaryModel();
    } catch (error) {
      host.showError(
        `Saved ${displayName} as the secondary model, but failed to apply it to this session: ${formatErrorMessage(
          error,
        )}`,
      );
      return;
    }
  }
  // Report the effective binding from the reloaded config, not the picked
  // value: DIMI_SECONDARY_MODEL / DIMI_SECONDARY_EFFORT override the recipe at
  // runtime, and the session binds the overlaid snapshot (mirrors how
  // /model displays the effective alias read back from the session).
  const effective = updatedConfig.secondaryModel;
  const envOverrides: string[] = [];
  if (effective?.provider !== undefined && effective.provider !== selected?.provider) {
    envOverrides.push(`DIMI_SECONDARY_PROVIDER=${effective.provider}`);
  }
  if (effective?.model !== undefined && effective.model !== selected?.model) {
    envOverrides.push(`DIMI_SECONDARY_MODEL=${effective.model}`);
  }
  if (effective?.defaultEffort !== undefined && effective.defaultEffort !== effort) {
    envOverrides.push(`DIMI_SECONDARY_EFFORT=${effective.defaultEffort}`);
  }
  if (envOverrides.length > 0 && effective?.model !== undefined) {
    const effectiveReference = configuredModelReference(
      host.state.appState.availableModels,
      effective.provider,
      effective.model,
    );
    const effectiveName = modelDisplayName(
      effectiveReference,
      host.state.appState.availableModels[effectiveReference],
    );
    host.showStatus(
      `Saved ${displayName} as the secondary model, but ${envOverrides.join(' and ')} ` +
        `overrides it at runtime — subagents bind ${effectiveName} until the env var is unset.`,
      'warning',
    );
    return;
  }
  host.showStatus(
    host.session === undefined
      ? `Secondary model set to ${displayName} with thinking ${effort}; applies to new sessions.`
      : `Secondary model set to ${displayName} with thinking ${effort}.`,
    'success',
  );
}

function showThemePicker(host: SlashCommandHost): void {
  host.mountEditorReplacement(
    new ThemeSelectorComponent({
      currentValue: host.state.appState.theme,
      onSelect: (value) => {
        host.restoreEditor();
        void applyThemeChoice(host, value);
      },
      onCancel: () => {
        host.restoreEditor();
      },
    }),
  );
}

async function applyThemeChoice(host: SlashCommandHost, theme: ThemeName): Promise<void> {
  if (theme === host.state.appState.theme) {
    if (theme === 'auto') host.refreshTerminalThemeTracking();
    host.showStatus(`Theme unchanged: "${theme}".`);
    return;
  }

  // Validate custom themes up front so a missing / malformed file reports an
  // error instead of silently persisting a name that resolves to the dark
  // fallback.
  if (!isBuiltInTheme(theme)) {
    const palette = await loadCustomThemeMerged(theme);
    if (palette === null) {
      host.showStatus(`Theme "${theme}" could not be loaded.`, 'error');
      return;
    }
  }

  try {
    await saveTuiConfig({
      ...currentTuiConfig(host),
      theme,
    });
  } catch (error) {
    host.showStatus(`Failed to save theme: ${formatErrorMessage(error)}`, 'error');
    return;
  }

  const resolved =
    theme === 'auto' ? (currentTheme.palette === lightColors ? 'light' : 'dark') : undefined;
  await host.applyTheme(theme, resolved);
  host.refreshTerminalThemeTracking();
  host.track('theme_switch', { theme });
  const detail = theme === 'auto' ? ` (tracking terminal; current: ${resolved})` : '';
  host.showStatus(`Theme set to "${theme}"${detail}.`);
}

export function showPermissionPicker(host: SlashCommandHost): void {
  host.mountEditorReplacement(
    new PermissionSelectorComponent({
      currentValue: host.state.appState.permissionMode,
      onSelect: (value) => {
        host.restoreEditor();
        void applyPermissionChoice(host, value, true);
      },
      onSessionOnlySelect: (value) => {
        host.restoreEditor();
        void applyPermissionChoice(host, value, false);
      },
      onCancel: () => {
        host.restoreEditor();
      },
    }),
  );
}

export function showUpdatePreferencePicker(host: SlashCommandHost): void {
  host.mountEditorReplacement(
    new UpdatePreferenceSelectorComponent({
      currentValue: host.state.appState.upgrade.autoInstall,
      onSelect: (value) => {
        host.restoreEditor();
        void applyUpdatePreferenceChoice(host, value);
      },
      onCancel: () => {
        host.restoreEditor();
      },
    }),
  );
}

export function showBusyInputModePicker(host: SlashCommandHost): void {
  host.mountEditorReplacement(
    new BusyInputModeSelectorComponent({
      currentValue: host.state.appState.busyInputMode ?? DEFAULT_TUI_CONFIG.busyInputMode,
      onSelect: (value) => {
        host.restoreEditor();
        void applyBusyInputModeChoice(host, value);
      },
      onCancel: () => {
        host.restoreEditor();
      },
    }),
  );
}

export async function showExperimentsPanel(host: SlashCommandHost): Promise<void> {
  let features: readonly ExperimentalFeatureState[];
  try {
    features = await host.harness.getExperimentalFeatures();
  } catch (error) {
    host.showError(`Failed to load experimental features: ${formatErrorMessage(error)}`);
    return;
  }
  mountExperimentsPanel(host, features);
}

export async function applyExperimentalFeatureChanges(
  host: SlashCommandHost,
  changes: readonly ExperimentalFeatureDraftChange[],
): Promise<void> {
  if (changes.length === 0) {
    host.showStatus('No experimental feature changes to apply.', 'textMuted');
    return;
  }

  const experimental: Record<string, boolean> = {};
  for (const change of changes) {
    experimental[change.id] = change.enabled;
  }

  try {
    await host.harness.setConfig({ experimental });
    const features = await host.harness.getExperimentalFeatures();
    setExperimentalFeatures(features);
    host.refreshSlashCommandAutocomplete();
    host.restoreEditor();
    if (host.session !== undefined) {
      await host.session.reloadSession();
      await host.reloadCurrentSessionView(
        host.session,
        'Experimental features updated. Session reloaded.',
      );
    } else {
      host.showStatus('Experimental features updated.', 'success');
    }
    host.track('experimental_features_apply', { changed: changes.length });
  } catch (error) {
    host.showError(`Failed to update experimental features: ${formatErrorMessage(error)}`);
  }
}

function mountExperimentsPanel(
  host: SlashCommandHost,
  features: readonly ExperimentalFeatureState[],
): void {
  host.mountEditorReplacement(
    new ExperimentsSelectorComponent({
      features,
      onApply: (changes) => {
        void applyExperimentalFeatureChanges(host, changes);
      },
      onCancel: () => {
        host.restoreEditor();
      },
    }),
  );
}

type UpdatePreferenceHost = {
  readonly state: {
    readonly appState: Pick<
      SlashCommandHost['state']['appState'],
      | 'theme'
      | 'editorCommand'
      | 'notifications'
      | 'upgrade'
      | 'busyInputMode'
      | 'disablePasteBurst'
      | 'statusLine'
    >;
  };
  setAppState(
    patch: Partial<Pick<SlashCommandHost['state']['appState'], 'upgrade' | 'busyInputMode'>>,
  ): void;
  showStatus(msg: string, color?: string): void;
  track: SlashCommandHost['track'];
};

export async function applyUpdatePreferenceChoice(
  host: UpdatePreferenceHost,
  autoInstall: boolean,
): Promise<void> {
  if (autoInstall === host.state.appState.upgrade.autoInstall) {
    host.showStatus(`Automatic updates already ${autoInstall ? 'enabled' : 'disabled'}.`);
    return;
  }

  const upgrade = { autoInstall };
  try {
    await saveTuiConfig({
      ...currentTuiConfig(host as unknown as SlashCommandHost),
      upgrade,
    });
  } catch (error) {
    host.showStatus(
      `Failed to save automatic update setting: ${formatErrorMessage(error)}`,
      'error',
    );
    return;
  }

  host.setAppState({ upgrade });
  host.track('upgrade_preference_changed', { auto_install: autoInstall });
  host.showStatus(`Automatic updates ${autoInstall ? 'enabled' : 'disabled'}.`);
}

export async function applyBusyInputModeChoice(
  host: UpdatePreferenceHost,
  mode: BusyInputMode,
): Promise<void> {
  const current = host.state.appState.busyInputMode ?? DEFAULT_TUI_CONFIG.busyInputMode;
  if (mode === current) {
    host.showStatus(
      mode === 'steer'
        ? 'Busy input already set to steer (Enter injects mid-turn).'
        : 'Busy input already set to queue (Enter waits; Ctrl-S steers).',
    );
    return;
  }

  try {
    await saveTuiConfig({
      ...currentTuiConfig(host as unknown as SlashCommandHost),
      busyInputMode: mode,
    });
  } catch (error) {
    host.showStatus(`Failed to save busy input setting: ${formatErrorMessage(error)}`, 'error');
    return;
  }

  host.setAppState({ busyInputMode: mode });
  host.track('busy_input_mode_changed', { mode });
  host.showStatus(
    mode === 'steer'
      ? 'Busy input: Enter steers immediately while the agent is working.'
      : 'Busy input: Enter queues; use Ctrl-S to steer immediately.',
  );
}

/**
 * `/permission [<mode>]` — with an argument, applies the mode to the current
 * session and saves it as the default for new sessions (`default_permission_mode`);
 * without one, opens the picker (Enter saves the default, Alt+S is session-only).
 */
export async function handlePermissionCommand(
  host: SlashCommandHost,
  args: string,
): Promise<void> {
  const arg = args.trim().toLowerCase();
  if (arg.length === 0) {
    showPermissionPicker(host);
    return;
  }
  if (arg === 'manual' || arg === 'yolo' || arg === 'auto') {
    await applyPermissionChoice(host, arg, true);
    return;
  }
  host.showError(`Unknown permission mode: ${arg}. Use manual, yolo, or auto.`);
}

export async function applyPermissionChoice(
  host: SlashCommandHost,
  mode: PermissionMode,
  persistDefault: boolean,
): Promise<void> {
  if (mode === host.state.appState.permissionMode) {
    host.showStatus(`Permission mode unchanged: ${mode}.`);
    return;
  }

  if (persistDefault) {
    if (!(await applyPermissionModeWithDefault(host, mode))) return;
    host.showNotice(
      `Permission mode: ${mode}`,
      'Saved as the default for new sessions (Alt+S applies to this session only).',
    );
    return;
  }

  try {
    await host.requireSession().setPermission(mode);
  } catch (error) {
    const msg = formatErrorMessage(error);
    host.showError(`Failed to set permission mode: ${msg}`);
    return;
  }

  host.setAppState({ permissionMode: mode });
  host.showNotice(`Permission mode: ${mode}`, 'Applied to this session only.');
}

export function showSettingsSelector(host: SlashCommandHost): void {
  host.mountEditorReplacement(
    new SettingsSelectorComponent({
      onSelect: (value) => {
        handleSettingsSelection(host, value);
      },
      onCancel: () => {
        host.restoreEditor();
      },
    }),
  );
}

function handleSettingsSelection(host: SlashCommandHost, value: SettingsSelection): void {
  host.restoreEditor();
  switch (value) {
    case 'model':
      showModelPicker(host);
      return;
    case 'permission':
      showPermissionPicker(host);
      return;
    case 'theme':
      showThemePicker(host);
      return;
    case 'editor':
      showEditorPicker(host);
      return;
    case 'busy-input':
      showBusyInputModePicker(host);
      return;
    case 'experiments':
      void showExperimentsPanel(host);
      return;
    case 'upgrade':
      showUpdatePreferencePicker(host);
      return;
    case 'usage':
      void showUsage(host);
      return;
  }
}
