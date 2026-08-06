import { createDecorator } from "#/_base/di/instantiation";
import type {
  BeforeExecuteDecision,
  ResolvedToolExecutionHookContext,
} from "#/agent/toolExecutor/toolHooks";

export type PlanData = null | {
  readonly id: string;
  readonly content: string;
  readonly path: string;
};

export type PlanFilePath = string | null;

export interface IAgentPlanService {
  readonly _serviceBrand: undefined;

  enter(id?: string, createFile?: boolean): Promise<void>;
  cancel(id?: string): void;
  clear(): Promise<void>;
  exit(id?: string): void;
  recordRevision(): Promise<void>;
  status(): Promise<PlanData>;

  /**
   * Applies plan-mode constraints to a tool executed by the Rust engine's
   * external-tool adapter. The normal TS loop reaches the same rules through
   * `onBeforeExecuteTool`; the Rust event loop owns tool lifecycle events, so
   * it asks the plan domain for only the adjudication result here.
   */
  adjudicateExternalTool(
    context: ResolvedToolExecutionHookContext,
  ): Promise<BeforeExecuteDecision | undefined>;
}

export const IAgentPlanService =
  createDecorator<IAgentPlanService>('agentPlanService');
