/**
 * `tools` domain (L7) — `IAgentOutputTool` contract (the `AgentOutput` tool).
 *
 * Lets the calling agent inspect a subagent's recent rendered output — the
 * same view a human gets from the TUI transcript: assistant text, thinking,
 * tool calls, and task progress, in time order. Subagents run fully
 * asynchronously, so this is the primary way to check on their progress
 * (alongside `WaitFor` for parking when there is nothing else to do).
 *
 * The public contract (input schema, `IAgentOutputTool`) lives here; the
 * implementation (`agentOutputTool.ts`) reads the target agent's `wire.jsonl`
 * tail and formats the latest renderable records.
 */

import { z } from 'zod';

import { createDecorator } from '#/_base/di/instantiation';
import { type AgentTool } from '#/tool/toolContract';

export const AgentOutputInputSchema = z.object({
  agent_id: z.string().describe('Agent ID of the subagent to inspect (returned by the Agent tool).'),
  tail_lines: z
    .number()
    .int()
    .min(1)
    .max(500)
    .optional()
    .describe(
      'How many of the most recent renderable records to show. Defaults to 60. The output is a transcript-style view of the subagent\'s latest activity.',
    ),
});

export type AgentOutputInput = z.infer<typeof AgentOutputInputSchema>;

export const AGENT_NOT_FOUND_MESSAGE = 'Agent instance not found.';

export interface IAgentOutputTool extends AgentTool<AgentOutputInput> {
  readonly _serviceBrand: undefined;
}

export const IAgentOutputTool = createDecorator<IAgentOutputTool>('agentOutputTool');
