/**
 * The aggregated klient contract — service wire name → method → zod
 * input/output schemas, across the core/session/agent scopes. The klient
 * factory validates every call against this table; transports never see it.
 * Event registrations live in the per-scope `events.ts` files alongside
 * their payload schemas.
 */

import type { KlientContract } from "./types.js";
import { agentActivityViewContract } from "./agent/activity.js";
import { agentRpcContract } from "./agent/rpc.js";
import {
  agentPlanContract,
  agentProfileContract,
  agentShellCommandContract,
  agentTaskContract,
  agentUsageContract,
} from "./agent/services.js";
import { catalogContract } from "./global/catalog.js";
import { configContract } from "./global/config.js";
import { envContract } from "./global/env.js";
import { flagsContract } from "./global/flags.js";
import { hostFsContract } from "./global/hostFs.js";
import { pluginsContract } from "./global/plugins.js";
import { sessionsContract } from "./global/sessions.js";
import { workspacesContract } from "./global/workspaces.js";
import { sessionApprovalContract } from "./session/approval.js";
import { sessionInteractionContract } from "./session/interaction.js";
import { sessionLifecycleContract } from "./session/lifecycle.js";
import { sessionMetadataContract } from "./session/metadata.js";
import { sessionQuestionContract } from "./session/question.js";

export const globalContract: KlientContract = {
  // core (app scope)
  sessionIndex: sessionsContract,
  workspaceService: workspacesContract,
  configService: configContract,
  modelResolver: catalogContract,
  flagService: flagsContract,
  pluginService: pluginsContract,
  hostFolderBrowser: hostFsContract,
  bootstrapService: envContract,
  // session scope (+ the app-registered lifecycle service)
  sessionLifecycleService: sessionLifecycleContract,
  sessionMetadata: sessionMetadataContract,
  sessionInteractionService: sessionInteractionContract,
  sessionApprovalService: sessionApprovalContract,
  sessionQuestionService: sessionQuestionContract,
  // agent scope
  agentRPCService: agentRpcContract,
  agentActivityView: agentActivityViewContract,
  agentShellCommandService: agentShellCommandContract,
  agentProfileService: agentProfileContract,
  agentUsageService: agentUsageContract,
  agentPlanService: agentPlanContract,
  agentTaskService: agentTaskContract,
};

export type {
  KlientContract,
  ProcedureContract,
  ServiceContract,
  StreamingProcedureContract,
} from "./types.js";
export { isStreamingContract } from "./types.js";
