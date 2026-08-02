/**
 * `SessionLegacyService` — kap-server-edge projection of the v1 session
 * actions `POST /sessions/{id}/profile` (`updateProfile` — title rename,
 * metadata merge, and the cross-domain `agent_config` patch) and
 * `GET /sessions/{id}/status` (`status`) on top of the native v2 engine
 * services.
 *
 * This is the v1 wire-compat adapter previously kept in agent-core-v2
 * (`src/app/sessionLegacy/`) — deliberately relocated to the kap-server edge
 * (same as `services/legacyStatus/`) so the core engine stays free of v1
 * wire-compatibility concerns. The thin pass-through actions (`fork` /
 * `compact` / `abort` / `archive`), the `:undo` action, and the
 * `/sessions/{id}/children` endpoints are NOT wrapped here: the route calls
 * the native engine services directly. Only `updateProfile` and `status`
 * hold real cross-domain adaptation (the `agent_config` patch and the
 * best-effort status rollup). The class is a
 * stateless dispatcher: it resolves the target session/agent per call
 * through the App `Scope`.
 */

import {
  Error2,
  ErrorCodes,
  ensureMainAgent,
  IAgentActivityView,
  IAgentContextSizeService,
  IAgentLifecycleService,
  IAgentPermissionModeService,
  IAgentPlanService,
  IAgentProfileService,
  IAgentSwarmService,
  IConfigService,
  IModelCatalog,
  ISessionContext,
  ISessionLifecycleService,
  ISessionMetadata,
  modelCapabilities,
  type IAgentScopeHandle,
  type PermissionMode,
  type Scope,
} from '@dimi-agent/agent-core-v2';
import type { SessionStatusResponse, UpdateSessionProfileRequest } from '@dimi-agent/protocol';

/** Wire fields projected by `updateProfile` — the `root` is the session cwd. */
export interface SessionLegacyWireFields {
  readonly id: string;
  readonly workspaceId: string;
  readonly root: string;
  readonly title?: string;
  readonly lastPrompt?: string;
  readonly createdAt: number;
  readonly updatedAt: number;
  readonly archived: boolean;
  readonly custom?: Record<string, unknown>;
}

export class SessionLegacyService {
  constructor(private readonly core: Scope) {}

  async updateProfile(
    sessionId: string,
    body: UpdateSessionProfileRequest,
  ): Promise<SessionLegacyWireFields> {
    const lifecycle = this.core.accessor.get(ISessionLifecycleService);
    const session = await lifecycle.resume(sessionId);
    if (session === undefined) {
      throw new Error2(ErrorCodes.SESSION_NOT_FOUND, `session ${sessionId} does not exist`);
    }
    const metadata = session.accessor.get(ISessionMetadata);

    if (typeof body.title === 'string') {
      await metadata.setTitle(body.title);
    }

    const metadataPatch = body.metadata;
    if (metadataPatch !== undefined && Object.keys(metadataPatch).length > 0) {
      await metadata.update({ custom: { ...(metadataPatch as Record<string, unknown>) } });
    }

    const agentConfig = body.agent_config;
    if (agentConfig !== undefined) {
      const agent = await this.resolveMainAgent(sessionId);
      await this.applyAgentConfig(agent, agentConfig);
    }

    const meta = await metadata.read();
    const ctx = session.accessor.get(ISessionContext);
    return {
      id: meta.id,
      workspaceId: ctx.workspaceId,
      root: ctx.cwd,
      title: meta.title,
      lastPrompt: meta.lastPrompt,
      createdAt: meta.createdAt,
      updatedAt: meta.updatedAt,
      archived: meta.archived,
      custom: meta.custom,
    };
  }

  private async applyAgentConfig(
    agent: IAgentScopeHandle,
    agentConfig: NonNullable<UpdateSessionProfileRequest['agent_config']>,
  ): Promise<void> {
    const profile = agent.accessor.get(IAgentProfileService);
    if (agentConfig.model !== undefined && agentConfig.model !== '') {
      await profile.setModel(agentConfig.model);
    }
    if (agentConfig.thinking !== undefined) {
      profile.setThinking(agentConfig.thinking);
    }
    if (agentConfig.permission_mode !== undefined) {
      agent.accessor
        .get(IAgentLifecycleService)
        .broadcastPermissionMode(agentConfig.permission_mode as PermissionMode);
    }
    if (agentConfig.plan_mode !== undefined) {
      const plan = agent.accessor.get(IAgentPlanService);
      const active = (await plan.status()) !== null;
      if (active !== agentConfig.plan_mode) {
        if (agentConfig.plan_mode) await plan.enter();
        else plan.exit();
      }
    }
    if (agentConfig.swarm_mode !== undefined) {
      const swarm = agent.accessor.get(IAgentSwarmService);
      if (swarm.isActive !== agentConfig.swarm_mode) {
        if (agentConfig.swarm_mode) swarm.enter('manual');
        else swarm.exit();
      }
    }
  }

  private async resolveMainAgent(sessionId: string): Promise<IAgentScopeHandle> {
    const session = await this.core.accessor.get(ISessionLifecycleService).resume(sessionId);
    if (session === undefined) {
      throw new Error2(ErrorCodes.SESSION_NOT_FOUND, `session ${sessionId} does not exist`);
    }
    return ensureMainAgent(session);
  }

  async status(sessionId: string): Promise<SessionStatusResponse> {
    const agent = await this.resolveMainAgent(sessionId);
    return this.assembleStatus(sessionId, agent);
  }

  private async assembleStatus(
    sessionId: string,
    agent: IAgentScopeHandle,
  ): Promise<SessionStatusResponse> {
    const profile = agent.accessor.get(IAgentProfileService);
    const contextSize = agent.accessor.get(IAgentContextSizeService);
    const permission = agent.accessor.get(IAgentPermissionModeService);
    const plan = agent.accessor.get(IAgentPlanService);
    const swarm = agent.accessor.get(IAgentSwarmService);

    const model = profile.getModel();
    const caps = profile.getModelCapabilities() as {
      max_context_tokens?: number;
      max_input_tokens?: number;
    };
    const maxTokens =
      model === ''
        ? resolveDefaultModelContextTokens(agent)
        : (caps.max_input_tokens ?? caps.max_context_tokens ?? 0);
    const tokens = contextSize.get().size;
    const planData = await plan.status();

    return {
      busy: this.readBusy(sessionId),
      model: model === '' ? undefined : model,
      thinking_level: profile.getEffectiveThinkingLevel(),
      permission: permission.mode,
      plan_mode: planData !== null,
      swarm_mode: swarm.isActive,
      context_tokens: tokens,
      max_context_tokens: maxTokens,
      context_usage: maxTokens > 0 ? Math.min(1, tokens / maxTokens) : 0,
    };
  }

  /**
   * The session's busy fact, derived on demand from the agents' activity
   * views (any active turn or background task). Nothing is booked at session
   * level — a cold session is simply not busy.
   */
  private readBusy(sessionId: string): boolean {
    const handle = this.core.accessor.get(ISessionLifecycleService).get(sessionId);
    if (handle === undefined) return false;
    for (const agent of handle.accessor.get(IAgentLifecycleService).list()) {
      const state = agent.accessor.get(IAgentActivityView).state();
      if (state.turn !== undefined || state.background.length > 0) return true;
    }
    return false;
  }

}

function resolveDefaultModelContextTokens(agent: IAgentScopeHandle): number {
  const defaultModel = agent.accessor.get(IConfigService).get<string>('defaultModel');
  const defaultProvider = agent.accessor.get(IConfigService).get<string>('defaultProvider');
  if (typeof defaultModel !== 'string' || defaultModel.length === 0) return 0;
  try {
    const reference =
      typeof defaultProvider === 'string' && defaultProvider.length > 0
        ? `${defaultProvider}/${defaultModel}`
        : defaultModel;
    const capabilities = modelCapabilities(agent.accessor.get(IModelCatalog).get(reference));
    return capabilities.max_input_tokens ?? capabilities.max_context_tokens;
  } catch {
    return 0;
  }
}
