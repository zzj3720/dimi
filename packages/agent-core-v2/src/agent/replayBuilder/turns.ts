import type { AgentReplayRecord } from './types';
import type { PromptOrigin } from '#/agent/contextMemory/types';

export function isUserVisiblePromptOrigin(origin: PromptOrigin | undefined): boolean {
  switch (origin?.kind) {
    case undefined:
    case 'user':
      return true;
    case 'skill_activation':
    case 'plugin_command':
      return origin.trigger === 'user-slash';
    case 'shell_command':
      return origin.phase === 'input';
    case 'system_trigger':
      return false;
    default:
      return false;
  }
}

export function isAgentReplayUserTurnRecord(record: AgentReplayRecord): boolean {
  if (record.type !== 'message' || record.message.role !== 'user') return false;
  return isUserVisiblePromptOrigin(record.message.origin);
}

export function limitAgentReplayByTurns(
  records: readonly AgentReplayRecord[],
  maxTurns?: number,
): readonly AgentReplayRecord[] {
  if (maxTurns === undefined) return records;
  if (maxTurns <= 0) return [];
  const starts = records.flatMap((record, index) =>
    isAgentReplayUserTurnRecord(record) ? [index] : [],
  );
  return starts.length <= maxTurns ? records : records.slice(starts[starts.length - maxTurns]);
}
