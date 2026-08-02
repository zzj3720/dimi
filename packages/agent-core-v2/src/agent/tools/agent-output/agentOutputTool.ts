/**
 * `tools` domain (L7) — `AgentOutputTool` implementation (the `AgentOutput`
 * tool).
 *
 * Returns a transcript-style view of a subagent's recent activity by reading
 * the tail of its `wire.jsonl` journal (`<sessionDir>/agents/<agentId>/`):
 * assistant text, thinking, tool calls, and task progress in time order —
 * the same surface a human sees in the TUI. Subagents run fully
 * asynchronously, so this is the primary progress-check tool for the calling
 * agent (park with `WaitFor` instead of polling when there is nothing else
 * to do).
 *
 * Registered via the module-level `registerAgentToolService(IAgentOutputTool,
 * AgentOutputTool)` at the bottom of this file — the same "import = register"
 * pattern used by every agent tool. Bound at Agent scope.
 */

import { open, type FileHandle } from 'node:fs/promises';
import { createInterface } from 'node:readline';
import { Readable } from 'node:stream';
import { finished } from 'node:stream/promises';

import { join } from 'pathe';

import { toInputJsonSchema } from '#/tool/input-schema';
import { matchesGlobRuleSubject } from '#/tool/rule-match';
import { type ExecutableToolResult, type ToolExecution } from '#/tool/toolContract';
import { registerAgentToolService } from '#/agent/toolRegistry/toolContribution';
import { IAgentContextSizeService } from '#/agent/contextSize/contextSize';
import { IAgentLoopService } from '#/agent/loop/loop';
import { IAgentProfileService } from '#/agent/profile/profile';
import { IAgentLifecycleService } from '#/session/agentLifecycle/agentLifecycle';
import { ISessionContext } from '#/session/sessionContext/sessionContext';

import {
  AGENT_NOT_FOUND_MESSAGE,
  AgentOutputInputSchema,
  IAgentOutputTool,
  type AgentOutputInput,
} from './agent-output';
import AGENT_OUTPUT_DESCRIPTION from './agent-output.md?raw';

const WIRE_FILENAME = 'wire.jsonl';
const TAIL_READ_BYTES = 256 * 1024;
const LINE_MAX_CHARS = 240;
const ARGS_MAX_CHARS = 120;
const PROMPT_MAX_CHARS = 300;

interface RenderableRecord {
  readonly timeMs: number;
  readonly turnStep: string;
  readonly lines: readonly string[];
}

export class AgentOutputTool implements IAgentOutputTool {
  declare readonly _serviceBrand: undefined;
  readonly name = 'AgentOutput' as const;
  readonly description: string = AGENT_OUTPUT_DESCRIPTION;
  readonly parameters: Record<string, unknown> = toInputJsonSchema(AgentOutputInputSchema);

  constructor(
    @IAgentLifecycleService private readonly lifecycle: IAgentLifecycleService,
    @ISessionContext private readonly session: ISessionContext,
  ) {}

  resolveExecution(args: AgentOutputInput): ToolExecution {
    return {
      description: `Reading output of agent ${args.agent_id}`,
      taskMode: 'control',
      approvalRule: this.name,
      matchesRule: (ruleArgs) => matchesGlobRuleSubject(ruleArgs, args.agent_id),
      execute: () => this.execute(args),
    };
  }

  private async execute(args: AgentOutputInput): Promise<ExecutableToolResult> {
    const target = this.lifecycle.get(args.agent_id);
    if (target === undefined) {
      return { isError: true, output: `${AGENT_NOT_FOUND_MESSAGE} agent_id=${args.agent_id}` };
    }

    const records = await this.readRecentRecords(args.agent_id, args.tail_lines ?? 60);

    const profileName =
      target.accessor.get(IAgentProfileService).data().profileName ?? 'agent';
    const status = target.accessor.get(IAgentLoopService).status();
    const contextSize = target.accessor.get(IAgentContextSizeService)?.get().size ?? 0;

    const header = [
      `agent_id: ${args.agent_id}`,
      `type: ${profileName}`,
      `status: ${status.state}${status.activeTurnId !== undefined ? ` · turn ${String(status.activeTurnId)}` : ''}`,
      `context: ${formatTokens(contextSize)}`,
    ].join(' · ');

    const lines: string[] = [header, ''];
    if (records.length === 0) {
      lines.push('(no activity recorded yet)');
    } else {
      for (const record of records) {
        const stamp = formatTime(record.timeMs);
        for (const line of record.lines) {
          lines.push(`${stamp} ${record.turnStep}${line}`);
        }
      }
    }
    return { output: lines.join('\n'), isError: false };
  }

  private async readRecentRecords(
    agentId: string,
    tailLines: number,
  ): Promise<readonly RenderableRecord[]> {
    const path = join(this.session.sessionDir, 'agents', agentId, WIRE_FILENAME);
    let file: FileHandle;
    try {
      file = await open(path, 'r');
    } catch (error) {
      if (isMissingPath(error)) return [];
      throw error;
    }

    let input: Readable | undefined;
    try {
      const size = (await file.stat()).size;
      const start = Math.max(0, size - TAIL_READ_BYTES);
      input =
        size === 0
          ? Readable.from([])
          : file.createReadStream({
              encoding: 'utf8',
              autoClose: false,
              start,
              end: size - 1,
            });
      const lines = createInterface({ input, crlfDelay: Infinity });
      const rendered: RenderableRecord[] = [];
      for await (const line of lines) {
        const record = toRenderable(line);
        if (record === undefined) continue;
        rendered.push(record);
        if (rendered.length > tailLines) rendered.shift();
      }
      return rendered;
    } finally {
      input?.destroy();
      if (input !== undefined) await finished(input, { cleanup: true }).catch(() => {});
      await file.close();
    }
  }
}

function toRenderable(line: string): RenderableRecord | undefined {
  const trimmed = line.trim();
  if (trimmed.length === 0) return undefined;
  let parsed: unknown;
  try {
    parsed = JSON.parse(trimmed) as unknown;
  } catch {
    return undefined;
  }
  if (typeof parsed !== 'object' || parsed === null) return undefined;
  const record = parsed as {
    readonly type?: unknown;
    readonly time?: unknown;
    readonly input?: unknown;
    readonly event?: {
      readonly type?: unknown;
      readonly turnId?: unknown;
      readonly step?: unknown;
      readonly part?: { readonly type?: unknown; readonly text?: unknown; readonly think?: unknown };
      readonly name?: unknown;
      readonly args?: unknown;
      readonly isError?: unknown;
    };
    readonly info?: {
      readonly taskId?: unknown;
      readonly description?: unknown;
      readonly status?: unknown;
    };
  };
  const timeMs = typeof record.time === 'number' ? normalizeTimestampMs(record.time) : undefined;
  const turnStep = turnStepLabel(record.event);
  switch (record.type) {
    case 'turn.prompt':
      return {
        timeMs: timeMs ?? 0,
        turnStep: '',
        lines: [`user: ${clip(textOfPrompt(record.input), PROMPT_MAX_CHARS)}`],
      };
    case 'context.append_loop_event': {
      const event = record.event;
      const eventType = event?.type;
      if (eventType === 'content.part' && typeof event?.part?.type === 'string') {
        const partType = event.part.type;
        const text =
          partType === 'think'
            ? typeof event.part.think === 'string'
              ? event.part.think
              : ''
            : typeof event.part.text === 'string'
              ? event.part.text
              : '';
        const label = partType === 'think' ? 'think' : 'assistant';
        return { timeMs: timeMs ?? 0, turnStep, lines: [`${label}: ${clip(text, LINE_MAX_CHARS)}`] };
      }
      if (eventType === 'tool.call' && typeof event?.name === 'string') {
        return {
          timeMs: timeMs ?? 0,
          turnStep,
          lines: [`tool: ${event.name}${argsLabel(event.args)}`],
        };
      }
      if (eventType === 'tool.result') {
        const done = event?.isError === true ? 'error' : 'done';
        return { timeMs: timeMs ?? 0, turnStep, lines: [`  → ${done}`] };
      }
      return undefined;
    }
    case 'task.started':
    case 'task.terminated': {
      const info = record.info;
      if (info === undefined) return undefined;
      const description = typeof info.description === 'string' ? info.description : '';
      const taskId = typeof info.taskId === 'string' ? info.taskId : '';
      const status = typeof info.status === 'string' ? info.status : '';
      const label =
        record.type === 'task.started'
          ? `task: ${clip(description, LINE_MAX_CHARS)} (running, ${taskId})`
          : `task: ${clip(description, LINE_MAX_CHARS)} (${status}, ${taskId})`;
      return { timeMs: timeMs ?? 0, turnStep: '', lines: [label] };
    }
    default:
      return undefined;
  }
}

function turnStepLabel(
  event: { readonly turnId?: unknown; readonly step?: unknown } | undefined,
): string {
  if (event === undefined) return '';
  const turn = typeof event.turnId === 'string' ? event.turnId : undefined;
  const step = typeof event.step === 'number' ? event.step : undefined;
  if (turn === undefined && step === undefined) return '';
  return `[turn ${turn ?? '?'} step ${step ?? '?'}] `;
}

function argsLabel(args: unknown): string {
  if (typeof args !== 'object' || args === null) return '';
  const text = JSON.stringify(args);
  return `(${clip(text, ARGS_MAX_CHARS)})`;
}

function textOfPrompt(input: unknown): string {
  if (!Array.isArray(input)) return '';
  return input
    .flatMap((part) =>
      typeof part === 'object' &&
      part !== null &&
      (part as { readonly type?: unknown }).type === 'text' &&
      typeof (part as { readonly text?: unknown }).text === 'string'
        ? [(part as { readonly text: string }).text]
        : [],
    )
    .join('')
    .replaceAll(/\s+/g, ' ')
    .trim();
}

function clip(text: string, max: number): string {
  const collapsed = text.replaceAll(/\s+/g, ' ').trim();
  if (collapsed.length <= max) return collapsed;
  return `${collapsed.slice(0, max)}…`;
}

function normalizeTimestampMs(value: number): number {
  return value > 1e12 ? Math.floor(value) : Math.floor(value * 1000);
}

function formatTime(timeMs: number): string {
  if (timeMs <= 0) return '';
  const date = new Date(timeMs);
  const pad = (n: number): string => String(n).padStart(2, '0');
  return `${pad(date.getHours())}:${pad(date.getMinutes())}:${pad(date.getSeconds())}`;
}

function formatTokens(size: number): string {
  if (size < 1024) return `${String(size)} tokens`;
  return `${(size / 1024).toFixed(1)}k tokens`;
}

function isMissingPath(error: unknown): boolean {
  return (
    typeof error === 'object' &&
    error !== null &&
    'code' in error &&
    (error as NodeJS.ErrnoException).code === 'ENOENT'
  );
}

registerAgentToolService(IAgentOutputTool, AgentOutputTool, {
  name: 'AgentOutput',
  domain: 'subagent',
});
