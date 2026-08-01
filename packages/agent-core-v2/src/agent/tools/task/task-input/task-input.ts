import { z } from 'zod';

import { createDecorator } from '#/_base/di/instantiation';
import { BACKGROUND_BASH_STDIN_FLAG_ID } from '#/agent/task/flag';
import { IAgentTaskService } from '#/agent/task/task';
import { registerAgentToolService } from '#/agent/toolRegistry/toolContribution';
import { IFlagService } from '#/app/flag/flag';
import { toInputJsonSchema } from '#/tool/input-schema';
import { literalRulePattern, matchesGlobRuleSubject } from '#/tool/rule-match';
import type { AgentTool, ToolExecution } from '#/tool/toolContract';

export const TaskInputSchema = z.object({
  task_id: z.string().describe('The background Bash task ID to write to.'),
  input: z.string().describe('UTF-8 text to write verbatim. Include a newline when needed.'),
  close_stdin: z.boolean().optional().describe('Close stdin after this write to send EOF.'),
});

export type TaskInput = z.infer<typeof TaskInputSchema>;

export interface ITaskInputTool extends AgentTool<TaskInput> {
  readonly _serviceBrand: undefined;
}
export const ITaskInputTool = createDecorator<ITaskInputTool>('taskInputTool');

export class TaskInputTool implements ITaskInputTool {
  declare readonly _serviceBrand: undefined;
  readonly name = 'TaskInput' as const;
  readonly description =
    'Write UTF-8 text to a background Bash task started with stdin_mode="pipe". Include a newline when the program expects Enter; set close_stdin=true after the final write to send EOF. This is a raw pipe, not a terminal (PTY).';
  readonly parameters: Record<string, unknown> = toInputJsonSchema(TaskInputSchema);

  constructor(@IAgentTaskService private readonly tasks: IAgentTaskService) {}

  resolveExecution(args: TaskInput): ToolExecution {
    return {
      description: `Writing to task ${args.task_id}`,
      taskMode: 'control',
      display: {
        kind: 'generic',
        summary: `Write to task ${args.task_id}`,
        detail: `${String(Buffer.byteLength(args.input))} input bytes${args.close_stdin === true ? ' and EOF' : ''}`,
      },
      approvalRule: literalRulePattern(this.name, args.task_id),
      matchesRule: (ruleArgs) => matchesGlobRuleSubject(ruleArgs, args.task_id),
      execute: async () => {
        const result = await this.tasks.sendInput(args.task_id, {
          data: args.input,
          close: args.close_stdin,
        });
        return !result.ok
          ? { isError: true, output: result.error }
          : {
              isError: false,
              output:
                `task_id: ${args.task_id}\n` +
                `written_bytes: ${String(Buffer.byteLength(args.input))}\n` +
                `stdin_closed: ${String(args.close_stdin === true)}`,
            };
      },
    };
  }
}

registerAgentToolService(ITaskInputTool, TaskInputTool, {
  name: 'TaskInput',
  domain: 'agentTask',
  when: (accessor) => accessor.get(IFlagService).enabled(BACKGROUND_BASH_STDIN_FLAG_ID),
});
