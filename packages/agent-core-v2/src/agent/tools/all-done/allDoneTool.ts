/** `tools` domain (L7) — intentional Agent completion control tool. */

import { z } from 'zod';

import { createDecorator } from '#/_base/di/instantiation';
import { ALL_DONE_TOOL_NAME } from '#/agent/completion/completion';
import { IAgentProfileService } from '#/agent/profile/profile';
import { IAgentTaskService } from '#/agent/task/task';
import { registerAgentToolService } from '#/agent/toolRegistry/toolContribution';
import { toInputJsonSchema } from '#/tool/input-schema';
import {
  ToolAccesses,
  type AgentTool,
  type ToolExecution,
  type ToolResolutionContext,
} from '#/tool/toolContract';

import ALL_DONE_DESCRIPTION from './all-done.md?raw';

const AllDoneInputSchema = z.object({});
type AllDoneInput = z.infer<typeof AllDoneInputSchema>;

export interface IAllDoneTool extends AgentTool<AllDoneInput> {
  readonly _serviceBrand: undefined;
}
export const IAllDoneTool = createDecorator<IAllDoneTool>('allDoneTool');

export class AllDoneTool implements IAllDoneTool {
  declare readonly _serviceBrand: undefined;
  readonly name = ALL_DONE_TOOL_NAME;
  readonly description = ALL_DONE_DESCRIPTION;
  readonly parameters: Record<string, unknown> = toInputJsonSchema(AllDoneInputSchema);

  constructor(
    @IAgentTaskService private readonly tasks: Pick<IAgentTaskService, 'list'>,
  ) {}

  resolveExecution(_args: AllDoneInput, context?: ToolResolutionContext): ToolExecution {
    if (context?.toolCalls.length !== 1 || context.toolCalls[0]?.name !== this.name) {
      return {
        output: 'AllDone must be the only tool call in its round.',
        isError: true,
      };
    }
    const activeTasks = this.tasks.list(true);
    if (activeTasks.length > 0) {
      const summary = activeTasks.map((task) => `${task.taskId} (${task.status})`).join(', ');
      return {
        output: `AllDone cannot complete while background tasks are active: ${summary}. Continue monitoring them or call WaitFor.`,
        isError: true,
      };
    }
    return {
      accesses: ToolAccesses.none(),
      taskMode: 'control',
      approvalRule: this.name,
      execute: async () => ({ output: 'All work is complete.', stopTurn: true }),
    };
  }
}

registerAgentToolService(IAllDoneTool, AllDoneTool, {
  name: ALL_DONE_TOOL_NAME,
  domain: 'completion',
  when: (accessor) => accessor.get(IAgentProfileService).data().profileName !== undefined,
});
