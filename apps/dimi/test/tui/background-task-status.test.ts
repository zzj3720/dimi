import type { BackgroundTaskInfo, BackgroundTaskStatus } from '@dimi-agent/dimi-sdk';
import { describe, expect, it } from 'vitest';

import {
  formatBackgroundTaskTranscript,
  shouldShowBackgroundTaskTranscript,
} from '@/tui/utils/background-task-status';

function task(overrides: Partial<BackgroundTaskInfo> = {}): BackgroundTaskInfo {
  const taskId = overrides.taskId ?? 'bash-abcd1234';
  const kind = overrides.kind ?? (taskId.startsWith('agent-') ? 'agent' : 'process');
  const base = {
    taskId,
    kind,
    description: 'dev server',
    status: 'running',
    startedAt: Date.now() - 1000,
    endedAt: null,
    ...overrides,
  };
  if (kind === 'agent') {
    return {
      ...base,
      kind: 'agent',
      agentId: 'agent-child',
      subagentType: 'coder',
      ...overrides,
    } as BackgroundTaskInfo;
  }
  if (kind === 'tool') {
    return {
      ...base,
      kind: 'tool',
      turnId: 1,
      toolCallId: 'call-deadbeef',
      toolName: 'mcp__example__slow_probe',
      autoWaitTimeoutSeconds: 20,
      ...overrides,
    } as BackgroundTaskInfo;
  }
  if (kind === 'question') {
    return {
      ...base,
      kind: 'question',
      questionCount: 1,
      ...overrides,
    } as BackgroundTaskInfo;
  }
  return {
    ...base,
    kind: 'process',
    command: 'npm run dev',
    pid: 1234,
    exitCode: null,
    ...overrides,
  } as BackgroundTaskInfo;
}

describe('formatBackgroundTaskTranscript', () => {
  it('renders a bash started entry', () => {
    const data = formatBackgroundTaskTranscript(task({ status: 'running' }));
    expect(data.phase).toBe('started');
    expect(data.headline).toContain('bash task started');
    expect(data.detail).toBe('dev server');
  });

  it('renders an agent started entry', () => {
    const data = formatBackgroundTaskTranscript(
      task({ taskId: 'agent-deadbeef', status: 'running' }),
    );
    expect(data.headline).toContain('agent task started');
  });

  it('renders a question started entry', () => {
    const data = formatBackgroundTaskTranscript(
      task({
        taskId: 'question-deadbeef',
        kind: 'question',
        questionCount: 1,
        status: 'running',
      }),
    );
    expect(data.headline).toContain('question task started');
  });

  it('renders a tool started entry', () => {
    const data = formatBackgroundTaskTranscript(
      task({
        taskId: 'tool-deadbeef',
        kind: 'tool',
        turnId: 1,
        toolCallId: 'call-deadbeef',
        toolName: 'mcp__example__slow_probe',
        autoWaitTimeoutSeconds: 20,
        status: 'running',
      }),
    );
    expect(data.headline).toContain('tool task started');
  });

  it('renders a completed entry with exit code in detail', () => {
    const data = formatBackgroundTaskTranscript(
      task({ status: 'completed', exitCode: 0, endedAt: Date.now() }),
    );
    expect(data.phase).toBe('completed');
    expect(data.headline).toContain('completed');
    expect(data.detail).toContain('exit 0');
  });

  it('renders a failed entry with non-zero exit', () => {
    const data = formatBackgroundTaskTranscript(
      task({ status: 'failed', exitCode: 2, endedAt: Date.now() }),
    );
    expect(data.phase).toBe('failed');
    expect(data.headline).toContain('failed');
    expect(data.detail).toContain('exit 2');
  });

  it('renders a killed entry with stop reason', () => {
    const data = formatBackgroundTaskTranscript(
      task({ status: 'killed', stopReason: 'user', endedAt: Date.now() }),
    );
    expect(data.phase).toBe('failed');
    expect(data.headline).toContain('stopped');
    expect(data.detail).toContain('user');
  });

  it('renders a lost entry with restart note', () => {
    const data = formatBackgroundTaskTranscript(task({ status: 'lost', endedAt: Date.now() }));
    expect(data.phase).toBe('failed');
    expect(data.headline).toContain('lost');
    expect(data.detail).toContain('session restarted');
  });

  it('surfaces timeout stop reason for agent deadlines', () => {
    const data = formatBackgroundTaskTranscript(
      task({
        taskId: 'agent-aaaaaaaa',
        status: 'timed_out',
        endedAt: Date.now(),
      }),
    );
    expect(data.detail).toContain('timed out');
  });

  it('handles every BackgroundTaskStatus without throwing', () => {
    const statuses: BackgroundTaskStatus[] = [
      'running',
      'completed',
      'failed',
      'timed_out',
      'killed',
      'lost',
    ];
    for (const status of statuses) {
      expect(() => formatBackgroundTaskTranscript(task({ status }))).not.toThrow();
    }
  });
});

describe('shouldShowBackgroundTaskTranscript', () => {
  it('shows background process and question lifecycle cards (including started)', () => {
    expect(shouldShowBackgroundTaskTranscript(task({ status: 'running' }))).toBe(true);
    expect(
      shouldShowBackgroundTaskTranscript(
        task({ status: 'running', detached: true }),
      ),
    ).toBe(true);
    expect(
      shouldShowBackgroundTaskTranscript(task({ status: 'completed', endedAt: Date.now() })),
    ).toBe(true);
    expect(
      shouldShowBackgroundTaskTranscript(
        task({
          taskId: 'question-deadbeef',
          kind: 'question',
          questionCount: 1,
          status: 'running',
          detached: true,
        }),
      ),
    ).toBe(true);
  });

  it('hides foreground process cards', () => {
    expect(
      shouldShowBackgroundTaskTranscript(task({ status: 'running', detached: false })),
    ).toBe(false);
    expect(
      shouldShowBackgroundTaskTranscript(
        task({ status: 'completed', detached: false, endedAt: Date.now() }),
      ),
    ).toBe(false);
  });

  it('never shows agent cards via this helper', () => {
    expect(
      shouldShowBackgroundTaskTranscript(task({ taskId: 'agent-deadbeef', status: 'running' })),
    ).toBe(false);
    expect(
      shouldShowBackgroundTaskTranscript(
        task({ taskId: 'agent-deadbeef', status: 'failed', endedAt: Date.now() }),
      ),
    ).toBe(false);
  });

  it('hides foreground and successful background tool tasks', () => {
    expect(
      shouldShowBackgroundTaskTranscript(
        task({
          taskId: 'tool-fg',
          kind: 'tool',
          detached: false,
          status: 'running',
        }),
      ),
    ).toBe(false);
    expect(
      shouldShowBackgroundTaskTranscript(
        task({
          taskId: 'tool-bg-ok',
          kind: 'tool',
          detached: true,
          status: 'running',
        }),
      ),
    ).toBe(false);
    expect(
      shouldShowBackgroundTaskTranscript(
        task({
          taskId: 'tool-bg-ok',
          kind: 'tool',
          detached: true,
          status: 'completed',
          endedAt: Date.now(),
        }),
      ),
    ).toBe(false);
  });

  it('shows background tool failures only', () => {
    for (const status of ['failed', 'timed_out', 'killed', 'lost'] as const) {
      expect(
        shouldShowBackgroundTaskTranscript(
          task({
            taskId: 'tool-bg-bad',
            kind: 'tool',
            detached: true,
            status,
            endedAt: Date.now(),
          }),
        ),
      ).toBe(true);
    }
    // Foreground tool failure still belongs on the tool-call card, not a bg status line.
    expect(
      shouldShowBackgroundTaskTranscript(
        task({
          taskId: 'tool-fg-bad',
          kind: 'tool',
          detached: false,
          status: 'failed',
          endedAt: Date.now(),
        }),
      ),
    ).toBe(false);
  });
});
