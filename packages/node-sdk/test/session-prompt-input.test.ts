import { describe, expect, it, vi } from 'vitest';

import { SDKRpcClientBase, type SetSessionModelRpcInput, type SessionPromptRpcInput, type SetSessionPlanModeRpcInput, type SessionIdRpcInput } from '../src/rpc';
import { Session } from '../src/session';
import type { SessionPlan } from '../src/types';

class CapturingRpc extends SDKRpcClientBase {
  readonly promptCalls: unknown[] = [];
  readonly planModeCalls: unknown[] = [];
  readonly getPlanCalls: unknown[] = [];
  readonly clearPlanCalls: unknown[] = [];
  readonly setModelCalls: unknown[] = [];
  private setModelDelay: Promise<void> | undefined;
  private setModelCallCount = 0;
  private readonly setModelWaiters = new Set<() => void>();

  delaySetModelUntil(promise: Promise<void>): void {
    this.setModelDelay = promise;
  }

  waitForSetModelCalls(count: number): Promise<void> {
    if (this.setModelCallCount >= count) return Promise.resolve();
    return new Promise<void>((resolve) => {
      const check = () => {
        if (this.setModelCallCount < count) return;
        this.setModelWaiters.delete(check);
        resolve();
      };
      this.setModelWaiters.add(check);
    });
  }

  override async prompt(input: SessionPromptRpcInput): Promise<void> {
    this.promptCalls.push({ ...input, agentId: this.interactiveAgentId });
  }

  override async setPlanMode(input: SetSessionPlanModeRpcInput): Promise<void> {
    this.planModeCalls.push({ ...input, agentId: this.interactiveAgentId });
  }

  override async getPlan(input: SessionIdRpcInput): Promise<SessionPlan> {
    this.getPlanCalls.push({ ...input, agentId: this.interactiveAgentId });
    return null;
  }

  override async clearPlan(input: SessionIdRpcInput): Promise<void> {
    this.clearPlanCalls.push({ ...input, agentId: this.interactiveAgentId });
  }

  override async setModel(input: SetSessionModelRpcInput): Promise<{ model: string }> {
    this.setModelCallCount += 1;
    for (const waiter of this.setModelWaiters) waiter();
    if (this.setModelDelay !== undefined) await this.setModelDelay;
    this.setModelCalls.push({ ...input, agentId: this.interactiveAgentId });
    return { model: input.model };
  }
}

describe('Session prompt input', () => {
  it('passes multimodal parts to the runtime', async () => {
    const prompt = vi.fn(async () => {});
    const session = new Session({
      id: 'ses_multimodal_prompt',
      workDir: '/tmp/work',
      rpc: { prompt } as unknown as SDKRpcClientBase,
    });
    const input = [
      { type: 'text', text: 'describe these' },
      { type: 'image_url', imageUrl: { url: 'data:image/png;base64,AAAA' } },
      { type: 'video_url', videoUrl: { url: 'ms://file-123', id: 'file-123' } },
    ] as const;

    await session.prompt(input);

    expect(prompt).toHaveBeenCalledWith({ sessionId: 'ses_multimodal_prompt', input });
  });

  it('starts btw and returns the forked agent id', async () => {
    const startBtw = vi.fn(async () => 'agent-btw');
    const session = new Session({
      id: 'ses_btw_start',
      workDir: '/tmp/work',
      rpc: { startBtw } as unknown as SDKRpcClientBase,
    });

    await expect(session.startBtw()).resolves.toBe('agent-btw');
    expect(startBtw).toHaveBeenCalledWith({ sessionId: 'ses_btw_start' });
  });

  it('scopes an interactive agent across awaited session operations', async () => {
    const rpc = new CapturingRpc();
    const session = new Session({ id: 'ses_scoped_agent', workDir: '/tmp/work', rpc });

    await rpc.withInteractiveAgent('agent-btw', async () => {
      await session.prompt('side question');
      await session.setPlanMode(true);
      await session.getPlan();
      await session.clearPlan();
      await session.setPlanMode(false);
      expect(rpc.interactiveAgentId).toBe('agent-btw');
    });

    expect(rpc.interactiveAgentId).toBe('main');
    expect(rpc.promptCalls).toEqual([{ sessionId: 'ses_scoped_agent', agentId: 'agent-btw', input: [{ type: 'text', text: 'side question' }] }]);
    expect(rpc.planModeCalls).toEqual([
      { sessionId: 'ses_scoped_agent', agentId: 'agent-btw', enabled: true },
      { sessionId: 'ses_scoped_agent', agentId: 'agent-btw', enabled: false },
    ]);
    expect(rpc.getPlanCalls).toEqual([{ sessionId: 'ses_scoped_agent', agentId: 'agent-btw' }]);
    expect(rpc.clearPlanCalls).toEqual([{ sessionId: 'ses_scoped_agent', agentId: 'agent-btw' }]);
  });

  it('isolates overlapping interactive agent scopes while calls are pending', async () => {
    let release!: () => void;
    const rpc = new CapturingRpc();
    rpc.delaySetModelUntil(new Promise<void>((resolve) => { release = resolve; }));
    const session = new Session({ id: 'ses_overlapping_agents', workDir: '/tmp/work', rpc });

    const first = rpc.withInteractiveAgent('agent-a', () => session.setModel('model-a'));
    const second = rpc.withInteractiveAgent('agent-b', () => session.setModel('model-b'));
    await rpc.waitForSetModelCalls(2);
    release();
    await Promise.all([first, second]);

    expect(rpc.interactiveAgentId).toBe('main');
    expect(rpc.setModelCalls).toEqual(expect.arrayContaining([
      { sessionId: 'ses_overlapping_agents', agentId: 'agent-a', model: 'model-a' },
      { sessionId: 'ses_overlapping_agents', agentId: 'agent-b', model: 'model-b' },
    ]));
  });
});
