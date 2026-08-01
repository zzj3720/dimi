/**
 * Service name → DI token registry for the in-process dispatcher. Only leaf
 * modules are imported (tokens + types) — never the engine root barrel, so
 * hosting klient in-process does not force the full registration side effects
 * beyond what the host already bootstrapped.
 */

import type { ServiceIdentifier } from "@dimi-agent/agent-core-v2/_base/di/instantiation";
import { ISessionIndex } from "@dimi-agent/agent-core-v2/app/sessionIndex/sessionIndex";
import { IWorkspaceService } from "@dimi-agent/agent-core-v2/app/workspace/workspace";
import { IConfigService } from "@dimi-agent/agent-core-v2/app/config/config";
import { IModelCatalog } from "@dimi-agent/agent-core-v2/app/modelCatalog/catalog";
import { IFlagService } from "@dimi-agent/agent-core-v2/app/flag/flag";
import { IPluginService } from "@dimi-agent/agent-core-v2/app/plugin/plugin";
import { IBootstrapService } from "@dimi-agent/agent-core-v2/app/bootstrap/bootstrap";
import { IEventService } from "@dimi-agent/agent-core-v2/app/event/event";
import { IHostFolderBrowser } from "@dimi-agent/agent-core-v2/app/hostFolderBrowser/hostFolderBrowser";
import { ISessionLifecycleService } from "@dimi-agent/agent-core-v2/app/sessionLifecycle/sessionLifecycle";
import { ISessionMetadata } from "@dimi-agent/agent-core-v2/session/sessionMetadata/sessionMetadata";
import { ISessionInteractionService } from "@dimi-agent/agent-core-v2/session/interaction/interaction";
import { ISessionApprovalService } from "@dimi-agent/agent-core-v2/session/approval/approval";
import { ISessionQuestionService } from "@dimi-agent/agent-core-v2/session/question/question";
import { IAgentRPCService } from "@dimi-agent/agent-core-v2/agent/rpc/rpc";
import { IAgentActivityView } from "@dimi-agent/agent-core-v2/agent/activityView/activityView";
import { IAgentPlanService } from "@dimi-agent/agent-core-v2/agent/plan/plan";
import { IAgentProfileService } from "@dimi-agent/agent-core-v2/agent/profile/profile";
import { IAgentShellCommandService } from "@dimi-agent/agent-core-v2/agent/shellCommand/shellCommand";
import { IAgentTaskService } from "@dimi-agent/agent-core-v2/agent/task/task";
import { IAgentUsageService } from "@dimi-agent/agent-core-v2/agent/usage/usage";

/** Wire service name (decorator id string) → token. */
export const serviceTokens: Readonly<Record<string, ServiceIdentifier<unknown>>> = {
  sessionIndex: ISessionIndex,
  workspaceService: IWorkspaceService,
  configService: IConfigService,
  modelResolver: IModelCatalog,
  flagService: IFlagService,
  pluginService: IPluginService,
  hostFolderBrowser: IHostFolderBrowser,
  bootstrapService: IBootstrapService,
  sessionLifecycleService: ISessionLifecycleService,
  sessionMetadata: ISessionMetadata,
  sessionInteractionService: ISessionInteractionService,
  sessionApprovalService: ISessionApprovalService,
  sessionQuestionService: ISessionQuestionService,
  agentRPCService: IAgentRPCService,
  agentActivityView: IAgentActivityView,
  agentShellCommandService: IAgentShellCommandService,
  agentProfileService: IAgentProfileService,
  agentUsageService: IAgentUsageService,
  agentPlanService: IAgentPlanService,
  agentTaskService: IAgentTaskService,
};

export { IEventService };
