/**
 * Test doubles for the `state` domain: registers real `StateRegistry`
 * instances for the session- and agent-scope state service tokens.
 */

import type { ServiceRegistration } from '#/_base/di/test';
import { AgentStateService } from '#/agent/state/agentStateService';
import { IAgentStateService } from '#/agent/state/agentState';
import { SessionStateService } from '#/session/state/sessionStateService';
import { ISessionStateService } from '#/session/state/sessionState';

export function registerStateServices(reg: ServiceRegistration): void {
  reg.defineInstance(ISessionStateService, new SessionStateService());
  reg.defineInstance(IAgentStateService, new AgentStateService());
}
