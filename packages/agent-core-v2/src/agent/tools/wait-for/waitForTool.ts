/** `tools` domain (L7) — `WaitFor` generic current-Agent wait tool. */

import { z } from 'zod';

import { createDecorator } from '#/_base/di/instantiation';
import { toInputJsonSchema } from '#/tool/input-schema';
import { ToolAccesses, type AgentTool, type ToolExecution } from '#/tool/toolContract';
import { registerAgentToolService } from '#/agent/toolRegistry/toolContribution';
import { IAgentWaitService, WAIT_DEFAULT_SECONDS, WAIT_MAX_SECONDS } from '#/agent/wait/wait';

import WAIT_FOR_DESCRIPTION from './wait-for.md?raw';

export const WaitForInputSchema = z.object({
  reason: z.string().trim().min(1).describe('Why the current agent needs to wait.'),
  timeout_seconds: z
    .number()
    .int()
    .min(1)
    .max(WAIT_MAX_SECONDS)
    .default(WAIT_DEFAULT_SECONDS)
    .optional()
    .describe('Wait timeout in seconds. Defaults to 60; maximum 1800.'),
});

export type WaitForInput = z.infer<typeof WaitForInputSchema>;

export interface IWaitForTool extends AgentTool<WaitForInput> {
  readonly _serviceBrand: undefined;
}
export const IWaitForTool = createDecorator<IWaitForTool>('waitForTool');

export class WaitForTool implements IWaitForTool {
  declare readonly _serviceBrand: undefined;
  readonly name = 'WaitFor' as const;
  readonly description = WAIT_FOR_DESCRIPTION;
  readonly parameters: Record<string, unknown> = toInputJsonSchema(WaitForInputSchema);

  constructor(@IAgentWaitService private readonly waits: IAgentWaitService) {}

  resolveExecution(args: WaitForInput): ToolExecution {
    return {
      description: `Waiting: ${args.reason}`,
      accesses: ToolAccesses.none(),
      taskMode: 'control',
      stopBatchAfterThis: true,
      approvalRule: this.name,
      execute: async () => {
        const wait = await this.waits.start(
          args.reason,
          args.timeout_seconds ?? WAIT_DEFAULT_SECONDS,
        );
        return {
          output: JSON.stringify({
            status: 'waiting',
            wait_id: wait.waitId,
            reason: wait.reason,
            timeout_seconds: wait.timeoutSeconds,
            message: 'the agent will wake on any notification or on wait timeout',
          }),
          stopTurn: true,
        };
      },
    };
  }
}

registerAgentToolService(IWaitForTool, WaitForTool, { name: 'WaitFor', domain: 'wait' });
