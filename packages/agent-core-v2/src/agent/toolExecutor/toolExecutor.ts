/**
 * `toolExecutor` domain (L3) — Agent-scope tool execution contract.
 *
 * Defines the public execution surface for provider tool calls, the
 * before/will execution-interception events, the did execution hook,
 * tool-call result settlement, duplicate-call tagging for telemetry, and
 * preflight description extension points. Bound at Agent scope.
 */

import { createDecorator } from '#/_base/di/instantiation';
import type { IDisposable } from '#/_base/di/lifecycle';
import type { Event } from '#/_base/event';
import type { ToolResult, ToolSource } from '#/tool/toolContract';
import type {
  BeforeToolExecuteEvent,
  ToolDidExecuteContext,
  WillExecuteToolEvent,
} from '#/agent/toolExecutor/toolHooks';
import type { ToolCall } from '#/kosong/contract/message';
import type { ToolInputDisplay } from '#/tool/toolInputDisplay';
import type { OrderedHookSlot } from '#/hooks';
import type { LLMRequestTrace } from '#/kosong/contract/requestTrace';

export interface ToolCallStartedPayload {
  readonly toolCallId: string;
  readonly name: string;
  readonly args: unknown;
  readonly display?: ToolInputDisplay;
}

export interface ToolExecutorExecuteOptions {
  readonly signal: AbortSignal;
  readonly turnId: number;
  readonly trace?: LLMRequestTrace;
  readonly onToolCall?: (payload: ToolCallStartedPayload) => void;
}

export interface ToolExecutionResult {
  readonly toolCallId: string;
  readonly toolName: string;
  readonly result: ToolResult;
}

export interface ToolTaskLifecycleContext {
  readonly turnId: number;
  readonly toolCallId: string;
  readonly toolName: string;
  readonly description: string;
  readonly autoWaitTimeoutSeconds: number;
  readonly signal: AbortSignal;
}

export interface DetachedToolTask {
  readonly taskId: string;
  readonly toolCallId: string;
  readonly toolName: string;
  readonly autoWaitTimeoutSeconds: number;
}

export interface ToolTaskLifecycle {
  readonly taskId: string;
  readonly signal: AbortSignal;
  bindExecution(execution: Promise<void>): void;
  detach(): Promise<void>;
  settle(result: ToolResult): Promise<void>;
}

export interface ToolTaskLifecycleController {
  prepare(context: ToolTaskLifecycleContext): Promise<ToolTaskLifecycle>;
  beginAutoWait(turnId: number, tasks: readonly DetachedToolTask[]): Promise<void>;
}

export type MissingToolDescriber = (toolName: string) => string | undefined;
export type UnavailableToolDescriber = (toolName: string) => string | undefined;
export type ToolCallGuard = (tool: {
  readonly name: string;
  readonly source: ToolSource;
}) => string | undefined;

export type ToolCallDupType = 'same_step' | 'cross_step';

export interface IAgentToolExecutorService {
  readonly _serviceBrand: undefined;

  execute(
    calls: ToolCall[],
    options: ToolExecutorExecuteOptions,
  ): AsyncIterable<ToolExecutionResult>;

  /**
   * Veto event fired before an allowed decision is made on a tool call.
   * Listeners adjudicate through the event (`veto` / `allow` / `pass` /
   * `waitUntil`); there is no id and no ordering contract.
   */
  readonly onBeforeExecuteTool: Event<BeforeToolExecuteEvent>;

  /**
   * waitUntil participation event fired after a call is allowed and before
   * it is dispatched. Listeners attach readiness work via
   * `waitUntil(promise)`; the executor awaits all of it.
   */
  readonly onWillExecuteTool: Event<WillExecuteToolEvent>;

  readonly hooks: {
    readonly onDidExecuteTool: OrderedHookSlot<ToolDidExecuteContext>;
  };

  recordDupType(toolCallId: string, dupType: ToolCallDupType): void;

  registerToolCallGuard(guard: ToolCallGuard): IDisposable;
  registerUnavailableToolDescriber(describer: UnavailableToolDescriber): IDisposable;
  registerMissingToolDescriber(describer: MissingToolDescriber): IDisposable;

  registerTaskLifecycleController(controller: ToolTaskLifecycleController): IDisposable;
}

export const IAgentToolExecutorService = createDecorator<IAgentToolExecutorService>(
  'agentToolExecutorService',
);
