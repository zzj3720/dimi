import type {
  CreateSessionOptions,
  DimiHarness,
  ProviderAuthFacade,
  Session,
} from '@dimi-agent/dimi-sdk';

import type { SkillListSession } from '../commands';

import { AUTH_LOGIN_REQUIRED_STARTUP_NOTICE } from '../constant/dimi-tui';
import type { RefreshResult } from '../utils/refresh-providers';
import { providerModelToAlias } from '../utils/provider-model';
import { thinkingEffortFromConfig } from '../utils/thinking-config';
import type { SessionEventHandler } from './session-event-handler';
import type { AppState, DimiTUIOptions } from '../types';
import type { TUIState } from '../tui-state';

type MutableCreateSessionOptions = {
  -readonly [P in keyof CreateSessionOptions]: CreateSessionOptions[P];
};

export interface AuthFlowHost {
  state: TUIState;
  session: Session | undefined;
  readonly harness: DimiHarness;
  readonly options: DimiTUIOptions;

  setAppState(patch: Partial<AppState>): void;
  setStartupReady(): void;
  resetSessionRuntime(): void;
  setSession(session: Session): Promise<void>;
  syncRuntimeState(session?: Session): Promise<void>;
  closeSession(reason: string): Promise<void>;
  appendStartupNotice(extra: string): void;
  readonly sessionEventHandler: SessionEventHandler;
  fetchSessions(): Promise<void>;
  updateTerminalTitle(): void;
  refreshSkillCommands(session?: SkillListSession): Promise<void>;
  refreshPluginCommands(session?: Session): Promise<void>;
}

export class AuthFlowController {
  constructor(private readonly host: AuthFlowHost) {}

  async refreshAvailableModels(): Promise<void> {
    const models = await this.host.harness.auth.models();
    const providers = await this.host.harness.auth.providers();
    this.host.setAppState({
      availableModels: Object.fromEntries(
        models.map((model) => [`${model.provider}/${model.id}`, providerModelToAlias(model)]),
      ),
      availableProviders: Object.fromEntries(providers.map((provider) => [provider.id, provider])),
    });
  }

  enterLoginRequiredStartupState(): void {
    this.host.resetSessionRuntime();
    this.host.setAppState({
      sessionId: '',
      model: '',
      thinkingEffort: 'off',
      contextTokens: 0,
      maxContextTokens: 0,
      contextUsage: 0,
      sessionUsage: null,
      latestPromptUsage: null,
      sessionTitle: null,
    });
    this.host.appendStartupNotice(AUTH_LOGIN_REQUIRED_STARTUP_NOTICE);
    this.host.setStartupReady();
  }

  async activateModelAfterLogin(model: string, effort?: string): Promise<void> {
    const { host } = this;
    if (host.session !== undefined) {
      await host.session.setModel(model);
      if (effort !== undefined) {
        await host.session.setThinking(effort);
      }
      return;
    }

    const options: MutableCreateSessionOptions = {
      workDir: host.state.appState.workDir,
      model,
      thinking: effort,
      permission: host.options.startup.auto
        ? 'auto'
        : host.options.startup.yolo
        ? 'yolo'
        : undefined,
      planMode: host.state.appState.planMode ? true : undefined,
      // The post-login session is still the startup session: carry the
      // --agent/--agent-file binding resolved at launch.
      agentProfile: host.options.startup.agentProfile,
      agentFiles: host.options.startup.agentFiles?.length
        ? [...host.options.startup.agentFiles]
        : undefined,
    };
    if (host.state.appState.additionalDirs.length > 0) {
      options.additionalDirs = [...host.state.appState.additionalDirs];
    }
    const session = await host.harness.createSession(options);
    await host.setSession(session);
    host.setAppState({
      sessionId: session.id,
      sessionTitle: session.summary?.title ?? null,
    });
    await host.syncRuntimeState(session);
    host.sessionEventHandler.startSubscription();
    void host.fetchSessions();
    host.updateTerminalTitle();
    void host.refreshSkillCommands(host.session);
    void host.refreshPluginCommands(host.session);
  }

  async clearActiveSessionAfterLogout(): Promise<void> {
    await this.host.closeSession('logged out');
    this.host.resetSessionRuntime();
    this.host.setAppState({
      sessionId: '',
      model: '',
      sessionTitle: null,
    });
    await this.host.refreshSkillCommands();
    await this.host.refreshPluginCommands();
  }

  async refreshConfigAfterLogin(): Promise<void> {
    const { host } = this;
    const config = await host.harness.getConfig({ reload: true });
    await this.refreshAvailableModels();
    const availableModels = host.state.appState.availableModels;
    const availableProviders = host.state.appState.availableProviders;
    const configuredDefault =
      config.defaultProvider !== undefined && config.defaultModel !== undefined
        ? `${config.defaultProvider}/${config.defaultModel}`
        : config.defaultModel;
    const defaultModel = host.options.startup.model ?? configuredDefault;
    const selected = defaultModel !== undefined ? availableModels[defaultModel] : undefined;

    if (defaultModel === undefined || selected === undefined) {
      host.setAppState({ availableModels, availableProviders });
      return;
    }

    await this.activateModelAfterLogin(defaultModel, thinkingEffortFromConfig(config.thinking));
    const appStatePatch: Partial<AppState> = {
      availableModels,
      availableProviders,
      model: defaultModel,
      maxContextTokens: selected.maxContextSize,
    };
    host.setAppState(appStatePatch);
  }

  async refreshConfigAfterLogout(): Promise<void> {
    await this.refreshAvailableModels();
    this.host.setAppState({
      model: '',
      thinkingEffort: 'off',
      maxContextTokens: 0,
      contextUsage: 0,
      contextTokens: 0,
      sessionUsage: null,
      latestPromptUsage: null,
    });
  }

  /**
   * Re-fetch model lists from every provider whose runtime source supports
   * it, then update the local model projection. Runs best-effort: individual
   * provider failures are collected and returned instead of thrown.
   */
  async refreshProviderModels(): Promise<RefreshResult> {
    const { host } = this;
    const providers = await host.harness.auth.providers();
    const included = new Set(providers.map((provider) => provider.id));
    const before = groupModelIds(await host.harness.auth.models(), included);
    const refresh = await host.harness.auth.refreshModels({ force: true });
    const after = groupModelIds(await host.harness.auth.models(), included);
    const changed: RefreshResult['changed'] = [];
    const unchanged: string[] = [];
    for (const provider of included) {
      const previous = before.get(provider) ?? new Set();
      const next = after.get(provider) ?? new Set();
      const added = [...next].filter((id) => !previous.has(id)).length;
      const removed = [...previous].filter((id) => !next.has(id)).length;
      if (added === 0 && removed === 0) unchanged.push(provider);
      else {
        changed.push({
          providerId: provider,
          providerName: providers.find((item) => item.id === provider)?.name ?? provider,
          added,
          removed,
        });
      }
    }
    await this.refreshAvailableModels();
    return {
      changed,
      unchanged,
      failed: [...refresh.errors].map(([provider, error]) => ({
        provider,
        reason: error.message,
      })),
    };
  }
}

function groupModelIds(
  models: Awaited<ReturnType<ProviderAuthFacade['models']>>,
  included: ReadonlySet<string>,
): Map<string, Set<string>> {
  const out = new Map<string, Set<string>>();
  for (const model of models) {
    if (!included.has(model.provider)) continue;
    const ids = out.get(model.provider) ?? new Set<string>();
    ids.add(model.id);
    out.set(model.provider, ids);
  }
  return out;
}
