/**
 * Scenario: Agent task lifecycle, persistence, output retention, and teardown.
 *
 * Resolves the real `AgentTaskService` by interface, uses real `ProcessTask`
 * adapters where process signals are observable, and stubs only persistence,
 * wire, loop, and telemetry boundaries. Run with
 * `pnpm --filter @dimi-agent/agent-core-v2 exec vitest run test/agent/task/taskService.test.ts`.
 */

import { Readable, type Writable } from 'node:stream';

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { SyncDescriptor } from '#/_base/di/descriptors';
import { DisposableStore, toDisposable } from '#/_base/di/lifecycle';
import { ILogService } from '#/_base/log/log';
import { TestInstantiationService } from '#/_base/di/test';
import { IAgentConversationUndoParticipantRegistry } from '#/agent/contextMemory/conversationUndoParticipants';
import {
  IAgentContextInjectorService,
  type ContextInjectionContext,
  type ContextInjectionProvider,
} from '#/agent/contextInjector/contextInjector';
import { IAgentTaskService, type AgentTask, type AgentTaskInfo } from '#/agent/task/task';
import { DEFAULT_KILL_GRACE_MS } from '#/agent/task/configSection';
import { renderNotificationXml } from '#/agent/task/notificationXml';
import { AgentTaskService } from '#/agent/task/taskService';
import { ProcessTask } from '#/agent/tools/os/bash/process-task';
import type { IProcess } from '#/session/process/processRunner';
import { IConfigRegistry, IConfigService } from '#/app/config/config';
import { IAgentContextMemoryService } from '#/agent/contextMemory/contextMemory';
import type { ContextMessage } from '#/agent/contextMemory/types';
import { IAgentLoopService } from '#/agent/loop/loop';
import { IAgentScopeContext, makeAgentScopeContext } from '#/agent/scopeContext/scopeContext';
import { IAgentStateService } from '#/agent/state/agentState';
import { AgentStateService } from '#/agent/state/agentStateService';
import { ISessionContext, makeSessionContext } from '#/session/sessionContext/sessionContext';
import { IAtomicDocumentStore } from '#/persistence/interface/atomicDocumentStore';
import { IFileSystemStorageService } from '#/persistence/interface/storage';
import { ITelemetryService } from '#/app/telemetry/telemetry';
import { IAgentToolRegistryService } from '#/agent/toolRegistry/toolRegistry';
import {
  IAgentToolExecutorService,
  type ToolTaskLifecycleController,
} from '#/agent/toolExecutor/toolExecutor';
import { IAgentWaitService } from '#/agent/wait/wait';
import { createHooks } from '#/hooks';
import { IWireService, type WireHooks } from '#/wire/wire';
import { IEventBus } from '#/app/event/eventBus';
import { EventBusService } from '#/app/event/eventBusService';
import { ITaskService } from '#/app/task/task';
import { InMemoryStorageService } from '#/persistence/backends/memory/inMemoryStorageService';

import { stubLog } from '../../_base/log/stubs';
import { stubContextMemory } from '../contextMemory/stubs';
import { stubLoopWithHooks } from '../loop/stubs';
import type { TaskServiceTestManager } from './stubs';

function fakeProcessTask(): AgentTask {
  return {
    idPrefix: 'test',
    kind: 'process',
    description: 'fake process task',
    start: () => {},
    toInfo: (base) => ({ ...base, kind: 'process', command: 'echo', pid: 0, exitCode: null }),
  };
}

type RestoreHook = IWireService['hooks']['onDidRestore'];

function stubWireService(captureRestoreHook?: (hook: RestoreHook) => void): IWireService {
  const hooks = createHooks<WireHooks, keyof WireHooks>(['onDidRestore']);
  captureRestoreHook?.(hooks.onDidRestore);
  return {
    _serviceBrand: undefined,
    hooks,
    dispatch: () => {},
    seal: async () => {},
    restore: async () => {},
    flush: async () => {},
    getModel: (model) => model.initial() as never,
    subscribe: () => toDisposable(() => {}),
  } as IWireService;
}

describe('AgentTaskService', () => {
  let disposables: DisposableStore;
  let ix: TestInstantiationService;
  let eventBus: EventBusService;
  let injectionProviders: Map<string, ContextInjectionProvider>;
  let toolTaskController: ToolTaskLifecycleController | undefined;

  beforeEach(() => {
    disposables = new DisposableStore();
    ix = disposables.add(new TestInstantiationService());
    eventBus = disposables.add(new EventBusService());
    injectionProviders = new Map();
    toolTaskController = undefined;
    ix.stub(ILogService, stubLog());
    ix.stub(IAgentConversationUndoParticipantRegistry, {
      register: () => toDisposable(() => {}),
      list: () => [],
    });
    ix.stub(IWireService, stubWireService());
    ix.stub(IEventBus, eventBus);
    ix.stub(IAgentContextInjectorService, {
      register: (name, provider) => {
        injectionProviders.set(name, provider);
        return toDisposable(() => {
          injectionProviders.delete(name);
        });
      },
    });
    ix.stub(ITaskService, {
      run: () => {
        throw new Error('ITaskService.run is not used by this test');
      },
      defer: () => {
        throw new Error('ITaskService.defer is not used by this test');
      },
    });
    ix.stub(IAgentContextMemoryService, stubContextMemory());
    ix.stub(ITelemetryService, { track: () => {}, track2: () => {} });
    ix.stub(IAgentToolRegistryService, {
      register: () => toDisposable(() => {}),
    });
    ix.stub(IAgentToolExecutorService, {
      registerTaskLifecycleController: (controller: ToolTaskLifecycleController) => {
        toolTaskController = controller;
        return toDisposable(() => {});
      },
    } as unknown as IAgentToolExecutorService);
    ix.stub(IAgentWaitService, {
      active: () => null,
      start: async () => {
        throw new Error('not used');
      },
      startAutoWait: async () => null,
    });
    ix.stub(IAgentLoopService, stubLoopWithHooks());
    ix.stub(IConfigRegistry, { registerSection: () => {} });
    ix.stub(IConfigService, {
      get: (() => undefined) as IConfigService['get'],
    });
    ix.stub(
      ISessionContext,
      makeSessionContext({
        sessionId: 'test-session',
        workspaceId: 'test-ws',
        sessionDir: '/tmp/test-session',
        sessionScope: 'sessions/test-ws/test-session',
        cwd: '/tmp/test-session',
      }),
    );
    ix.stub(
      IAgentScopeContext,
      makeAgentScopeContext({
        agentId: 'main',
        agentScope: 'sessions/test-ws/test-session/agents/main',
      }),
    );
    ix.stub(IAtomicDocumentStore, {
      get: async () => undefined,
      set: async () => {},
      delete: async () => {},
      list: async () => [],
    });
    ix.stub(IFileSystemStorageService, {
      read: async () => undefined,
      readStream: async function* () {},
      write: async () => {},
      writeStream: async () => {},
      append: async () => {},
      list: async () => [],
      delete: async () => {},
      flush: async () => {},
      close: async () => {},
    });
    ix.set(IAgentStateService, new AgentStateService());
    ix.set(IAgentTaskService, new SyncDescriptor(AgentTaskService));
  });
  afterEach(() => disposables.dispose());

  it('registerTask / list / readOutput / stop', async () => {
    const svc = ix.get(IAgentTaskService);
    const id = svc.registerTask(fakeProcessTask());
    const listed = svc.list();
    expect(listed).toHaveLength(1);
    expect(listed[0]?.taskId).toBe(id);
    expect(listed[0]?.kind).toBe('process');
    expect(await svc.readOutput(id)).toBe('');
    await svc.stop(id);
  });

  it('records a generic tool before returning its lifecycle and persists detached completion', async () => {
    const dispatched: { type: string; payload: unknown }[] = [];
    let releaseWrite!: () => void;
    let releaseFlush!: () => void;
    let blockPersistence = true;
    const writeGate = new Promise<void>((resolve) => {
      releaseWrite = resolve;
    });
    const flushGate = new Promise<void>((resolve) => {
      releaseFlush = resolve;
    });
    ix.stub(IAtomicDocumentStore, {
      get: async () => undefined,
      set: async () => {
        if (blockPersistence) await writeGate;
      },
      delete: async () => {},
      list: async () => [],
    });
    ix.stub(IWireService, {
      ...stubWireService(),
      dispatch: (...ops: { type: string; payload: unknown }[]) => {
        dispatched.push(...ops);
      },
      flush: async () => {
        if (blockPersistence) await flushGate;
      },
    } as IWireService);

    const service = ix.get(IAgentTaskService);
    expect(toolTaskController).toBeDefined();
    let prepared = false;
    const pending = toolTaskController!
      .prepare({
        turnId: 7,
        toolCallId: 'call-long',
        toolName: 'LongTool',
        description: 'Running LongTool',
        autoWaitTimeoutSeconds: 20,
        signal: new AbortController().signal,
      })
      .then((lifecycle) => {
        prepared = true;
        return lifecycle;
      });

    await vi.waitFor(() => {
      expect(dispatched.map((op) => op.type)).toEqual(['task.started']);
    });
    expect(prepared).toBe(false);

    blockPersistence = false;
    releaseWrite();
    releaseFlush();
    const lifecycle = await pending;
    expect(service.getTask(lifecycle.taskId)).toMatchObject({
      kind: 'tool',
      turnId: 7,
      toolCallId: 'call-long',
      toolName: 'LongTool',
      detached: false,
      status: 'running',
    });

    await lifecycle.detach();
    await lifecycle.settle({ output: 'real final output' });

    expect(service.getTask(lifecycle.taskId)).toMatchObject({
      detached: true,
      status: 'completed',
    });
    expect(dispatched.map((op) => op.type)).toEqual([
      'task.started',
      'task.started',
      'task.terminated',
    ]);
    expect(dispatched.at(-1)?.payload).toMatchObject({ outputTail: 'real final output' });
  });

  function capturingWire(): { dispatched: { type: string; payload: unknown }[] } {
    const dispatched: { type: string; payload: unknown }[] = [];
    ix.stub(IWireService, {
      ...stubWireService(),
      dispatch: (...ops: { type: string; payload: unknown }[]) => {
        dispatched.push(...ops);
      },
    } as IWireService);
    return { dispatched };
  }

  function outputtingTask(output: string): AgentTask {
    return {
      ...fakeProcessTask(),
      start: async (sink) => {
        sink.appendOutput(output);
        await sink.settle({ status: 'completed' });
      },
    };
  }

  it('task.terminated dispatch carries the retained output tail as outputTail', async () => {
    const { dispatched } = capturingWire();
    const svc = ix.get(IAgentTaskService);
    const taskId = svc.registerTask(outputtingTask('line one\nline two\n'));

    await svc.wait(taskId, 1000);

    const terminated = dispatched.filter((op) => op.type === 'task.terminated');
    expect(terminated).toHaveLength(1);
    expect(terminated[0]?.payload).toMatchObject({
      info: { taskId, status: 'completed' },
      outputTail: 'line one\nline two\n',
    });
  });

  it('task.terminated outputTail is bounded to the last 4 KiB of retained output', async () => {
    const { dispatched } = capturingWire();
    const svc = ix.get(IAgentTaskService);
    const taskId = svc.registerTask(outputtingTask('x'.repeat(8 * 1024)));

    await svc.wait(taskId, 1000);

    const terminated = dispatched.find((op) => op.type === 'task.terminated');
    const payload = terminated?.payload as { outputTail?: string };
    expect(payload.outputTail).toBe('x'.repeat(4 * 1024));
  });

  it('task.terminated dispatch omits outputTail when the task produced no output', async () => {
    const { dispatched } = capturingWire();
    const svc = ix.get(IAgentTaskService);
    const taskId = svc.registerTask({
      ...fakeProcessTask(),
      start: async (sink) => {
        await sink.settle({ status: 'completed' });
      },
    });

    await svc.wait(taskId, 1000);

    const terminated = dispatched.find((op) => op.type === 'task.terminated');
    const payload = terminated?.payload as { outputTail?: string };
    expect(payload.outputTail).toBeUndefined();
  });

  function stubTaskConfig(value: unknown): void {
    ix.stub(IConfigService, {
      get: ((domain: string) => (domain === 'task' ? value : undefined)) as IConfigService['get'],
    });
  }

  function stubTaskWrites(): AgentTaskInfo[] {
    const writes: AgentTaskInfo[] = [];
    ix.stub(IAtomicDocumentStore, {
      get: async () => undefined,
      set: async <T>(_scope: string, _key: string, value: T) => {
        writes.push(value as AgentTaskInfo);
      },
      delete: async () => {},
      list: async () => [],
    });
    return writes;
  }

  function abortObservingTask(onAbort: (reason: unknown) => void): AgentTask {
    return {
      ...fakeProcessTask(),
      start: ({ signal }) => {
        if (signal.aborted) {
          onAbort(signal.reason);
          return;
        }
        signal.addEventListener('abort', () => onAbort(signal.reason));
      },
    };
  }

  it('stopAllOnExit suppresses and persists terminal state for detached tasks', async () => {
    const writes = stubTaskWrites();
    const svc = ix.get(IAgentTaskService);
    const first = svc.registerTask(fakeProcessTask());
    const second = svc.registerTask(fakeProcessTask());

    const stopped = await svc.stopAllOnExit('Session closed');

    expect(stopped.map((info) => info.taskId).toSorted()).toEqual([first, second].toSorted());
    for (const taskId of [first, second]) {
      const info = svc.getTask(taskId);
      expect(info?.status).toBe('killed');
      expect(info?.stopReason).toBe('Session closed');
      expect(info?.terminalNotificationSuppressed).toBe(true);
      const persisted = writes.filter((write) => write.taskId === taskId);
      expect(
        persisted.some(
          (write) => write.status === 'running' && write.terminalNotificationSuppressed === true,
        ),
      ).toBe(true);
      expect(persisted.at(-1)).toMatchObject({
        status: 'killed',
        terminalNotificationSuppressed: true,
      });
    }
  });

  it('stopAllOnExit does not persist a foreground-only task', async () => {
    const writes = stubTaskWrites();
    const svc = ix.get(IAgentTaskService);
    const taskId = svc.registerTask(fakeProcessTask(), { detached: false });

    await svc.stopAllOnExit('Session closed');

    expect(writes).toEqual([]);
    expect(svc.getTask(taskId)).toMatchObject({
      status: 'killed',
      detached: false,
      terminalNotificationSuppressed: undefined,
    });
  });

  it('dispose aborts live tasks as a last resort', async () => {
    const svc = ix.get(IAgentTaskService);
    let abortReason: unknown;
    svc.registerTask(
      abortObservingTask((reason) => (abortReason = reason)),
      {
        timeoutMs: 60_000,
      },
    );

    disposables.dispose();
    await Promise.resolve();

    expect(abortReason).toBe('Session closed');
  });

  it('scope disposal requests SIGKILL when a process ignores SIGTERM', async () => {
    const stdout = new Readable({ read() {} });
    const stderr = new Readable({ read() {} });
    let resolveWait!: (code: number) => void;
    const wait = new Promise<number>((resolve) => {
      resolveWait = resolve;
    });
    const kill = vi.fn(async (signal: NodeJS.Signals) => {
      if (signal !== 'SIGKILL') return;
      stdout.push(null);
      stderr.push(null);
      resolveWait(137);
    });
    const proc = {
      stdin: { write: vi.fn(), end: vi.fn() } as unknown as Writable,
      stdout,
      stderr,
      pid: 4244,
      exitCode: null,
      wait: () => wait,
      kill,
      dispose: vi.fn().mockResolvedValue(undefined),
    } as unknown as IProcess;
    const svc = ix.get(IAgentTaskService);
    svc.registerTask(new ProcessTask(proc, 'ignore-term', 'long-running process'));
    await Promise.resolve();

    disposables.dispose();
    await Promise.resolve();

    expect(kill).toHaveBeenNthCalledWith(1, 'SIGTERM');
    expect(kill).toHaveBeenNthCalledWith(2, 'SIGKILL');
  });

  it('stop requests force-stop when killGracePeriodMs is zero', async () => {
    stubTaskConfig({ killGracePeriodMs: 0 });
    const svc = ix.get(IAgentTaskService);
    let forceStopped = false;
    const taskId = svc.registerTask({
      ...fakeProcessTask(),
      start: () => new Promise<void>(() => {}),
      forceStop: async () => {
        forceStopped = true;
      },
    });

    const info = await svc.stop(taskId);

    expect(forceStopped).toBe(true);
    expect(info?.status).toBe('killed');
  });

  it('pins DEFAULT_KILL_GRACE_MS at 5000 (Rust engine parity)', () => {
    // F5 (review note): `DEFAULT_KILL_GRACE_MS` here and the Rust constant in
    // `crates/dimi-engine/src/tool.rs` are only comment-linked. The runner
    // wires this TS value into the engine turn input's `killGraceMs`, so the
    // Rust value is only a fallback for direct/nested construction — but if
    // the two ever diverge, this pin (plus the Rust-side pin in
    // tool.rs `default_kill_grace_ms_is_pinned_at_the_ts_default`) fails and
    // the mismatch is caught.
    expect(DEFAULT_KILL_GRACE_MS).toBe(5_000);
  });

  function mapBackedDocs(): IAtomicDocumentStore {
    const map = new Map<string, unknown>();
    return {
      _serviceBrand: undefined,
      get: async <T>(scope: string, key: string): Promise<T | undefined> =>
        map.get(`${scope}/${key}`) as T | undefined,
      set: async <T>(scope: string, key: string, value: T): Promise<void> => {
        map.set(`${scope}/${key}`, value);
      },
      delete: async (scope: string, key: string): Promise<void> => {
        map.delete(`${scope}/${key}`);
      },
      list: async (scope: string, prefix = ''): Promise<readonly string[]> =>
        [...map.keys()]
          .filter((key) => key.startsWith(`${scope}/${prefix}`))
          .map((key) => key.slice(scope.length + 1)),
    } as unknown as IAtomicDocumentStore;
  }

  function buildAgentIx(
    agentId: string,
    docs: IAtomicDocumentStore,
    bytes: IFileSystemStorageService,
    captureRestoreHook?: (hook: RestoreHook) => void,
  ): TestInstantiationService {
    const ix = disposables.add(new TestInstantiationService());
    ix.stub(ILogService, stubLog());
    ix.stub(IAgentConversationUndoParticipantRegistry, {
      register: () => toDisposable(() => {}),
      list: () => [],
    });
    ix.stub(IWireService, stubWireService(captureRestoreHook));
    ix.stub(IEventBus, disposables.add(new EventBusService()));
    ix.stub(IAgentContextInjectorService, {
      register: () => toDisposable(() => {}),
    });
    ix.stub(ITaskService, {
      run: () => {
        throw new Error('ITaskService.run is not used by this test');
      },
      defer: () => {
        throw new Error('ITaskService.defer is not used by this test');
      },
    });
    ix.stub(IAgentContextMemoryService, stubContextMemory());
    ix.stub(ITelemetryService, { track: () => {}, track2: () => {} });
    ix.stub(IAgentLoopService, stubLoopWithHooks());
    ix.stub(IAgentToolExecutorService, {
      registerTaskLifecycleController: () => toDisposable(() => {}),
    } as unknown as IAgentToolExecutorService);
    ix.stub(IConfigService, {
      get: (() => undefined) as IConfigService['get'],
    });
    ix.stub(
      ISessionContext,
      makeSessionContext({
        sessionId: 'test-session',
        workspaceId: 'test-ws',
        sessionDir: '/tmp/test-session',
        sessionScope: 'sessions/test-ws/test-session',
        cwd: '/tmp/test-session',
      }),
    );
    ix.stub(
      IAgentScopeContext,
      makeAgentScopeContext({
        agentId,
        agentScope: `sessions/test-ws/test-session/agents/${agentId}`,
      }),
    );
    ix.stub(IAtomicDocumentStore, docs);
    ix.stub(IFileSystemStorageService, bytes);
    ix.set(IAgentStateService, new AgentStateService());
    ix.set(IAgentTaskService, new SyncDescriptor(AgentTaskService));
    return ix;
  }

  it('restore touches only the agent own task records', async () => {
    const docs = mapBackedDocs();
    const bytes = new InMemoryStorageService();
    const subScope = 'sessions/test-ws/test-session/agents/agent-1';
    await docs.set(`${subScope}/tasks`, 'bash-abcdef01.json', {
      taskId: 'bash-abcdef01',
      kind: 'process',
      command: 'sleep 60',
      description: 'sub task',
      pid: 4242,
      startedAt: 1,
      endedAt: null,
      exitCode: null,
      status: 'running',
      detached: true,
    });

    const main = buildAgentIx('main', docs, bytes).get(IAgentTaskService) as TaskServiceTestManager;
    await main.loadFromDisk();
    const lost = await main.reconcile();

    expect(lost).toEqual([]);
    expect(main.list(false)).toEqual([]);
    const untouched = await docs.get<{ status: string }>(`${subScope}/tasks`, 'bash-abcdef01.json');
    expect(untouched?.status).toBe('running');

    const sub = buildAgentIx('agent-1', docs, bytes).get(
      IAgentTaskService,
    ) as TaskServiceTestManager;
    await sub.loadFromDisk();
    const subLost = await sub.reconcile();
    expect(subLost.map((info) => info.taskId)).toEqual(['bash-abcdef01']);
    expect(subLost[0]?.status).toBe('lost');
  });

  function compactionSummary(text: string): ContextMessage {
    return {
      role: 'user',
      content: [{ type: 'text', text }],
      toolCalls: [],
      origin: { kind: 'compaction_summary' },
    };
  }

  function publishCompactionSplice(): void {
    eventBus.publish({
      type: 'context.spliced',
      start: 0,
      deleteCount: 2,
      messages: [compactionSummary('Compacted summary.')],
    });
  }

  async function backgroundTaskReminder(
    context: ContextInjectionContext = {
      injectedPositions: [],
      lastInjectedAt: null,
      isNewTurn: false,
    },
  ): Promise<string | undefined> {
    const provider = injectionProviders.get('background_task_status');
    expect(provider).toBeDefined();
    const content = await provider!(context);
    return typeof content === 'string' ? content : undefined;
  }

  it('injects active background task status when compaction dropped the original launch context', async () => {
    const svc = ix.get(IAgentTaskService);
    const taskId = svc.registerTask(fakeProcessTask());

    expect(await backgroundTaskReminder()).toBeUndefined();

    publishCompactionSplice();

    const reminder = await backgroundTaskReminder();
    expect(reminder).toContain('The conversation was compacted');
    expect(reminder).toContain(
      'gone — but the tasks are still running from before. Do not start duplicates. Use TaskList to list them, TaskOutput for a non-blocking status/output snapshot',
    );
    expect(reminder).toContain('active_background_tasks: 1');
    expect(reminder).toContain(taskId);
    expect(reminder).toContain('TaskOutput');
    expect(reminder).toContain('TaskList');
    expect(reminder).toContain('TaskStop');
    expect(await backgroundTaskReminder()).toBeUndefined();

    await svc.stop(taskId);
  });

  it('does not carry post-compaction task reminder eligibility forward when no task is active', async () => {
    const svc = ix.get(IAgentTaskService);
    publishCompactionSplice();

    expect(await backgroundTaskReminder()).toBeUndefined();

    const taskId = svc.registerTask(fakeProcessTask());
    expect(await backgroundTaskReminder()).toBeUndefined();

    await svc.stop(taskId);
  });

  const MiB = 1024 * 1024;
  const LIMIT_BYTES = 16 * MiB;

  function streamingProcess(chunks: string[]): {
    proc: IProcess;
    kill: ReturnType<typeof vi.fn>;
  } {
    const stdout = Readable.from(chunks);
    const stderr = Readable.from([]);
    let resolveWait!: (code: number) => void;
    const waitP = new Promise<number>((resolve) => {
      resolveWait = resolve;
    });
    stdout.on('end', () => {
      resolveWait(0);
    });
    const kill = vi.fn(async (signal: string) => {
      stdout.destroy();
      resolveWait(signal === 'SIGKILL' ? 137 : 143);
    });
    const proc = {
      stdin: { write: vi.fn(), end: vi.fn() } as unknown as Writable,
      stdout,
      stderr,
      pid: 4242,
      exitCode: null,
      wait: () => waitP,
      kill,
      dispose: vi.fn().mockResolvedValue(undefined),
    } as unknown as IProcess;
    return { proc, kill };
  }

  function sigtermIgnoringProcess(chunks: string[]): {
    proc: IProcess;
    kill: ReturnType<typeof vi.fn>;
  } {
    const stdout = Readable.from(chunks);
    const stderr = Readable.from([]);
    let resolveWait!: (code: number) => void;
    const waitP = new Promise<number>((resolve) => {
      resolveWait = resolve;
    });
    stdout.on('end', () => {
      resolveWait(0);
    });
    const kill = vi.fn(async (signal: string) => {
      if (signal === 'SIGKILL') {
        stdout.destroy();
        resolveWait(137);
      }
    });
    const proc = {
      stdin: { write: vi.fn(), end: vi.fn() } as unknown as Writable,
      stdout,
      stderr,
      pid: 4243,
      exitCode: null,
      wait: () => waitP,
      kill,
      dispose: vi.fn().mockResolvedValue(undefined),
    } as unknown as IProcess;
    return { proc, kill };
  }

  function agentLikeTask(result: string, description: string): AgentTask {
    return {
      idPrefix: 'agent',
      kind: 'agent',
      description,
      start: async (sink) => {
        sink.appendOutput(result);
        await sink.settle({ status: 'completed' });
      },
      toInfo: (base) => ({ ...base, kind: 'agent' }),
    };
  }

  async function waitForTerminal(
    svc: IAgentTaskService,
    taskId: string,
    timeoutMs = 30_000,
  ): Promise<AgentTaskInfo | undefined> {
    const deadline = Date.now() + timeoutMs;
    while (Date.now() <= deadline) {
      const info = await svc.wait(taskId, 5);
      if (
        info?.status === 'completed' ||
        info?.status === 'failed' ||
        info?.status === 'timed_out' ||
        info?.status === 'killed' ||
        info?.status === 'lost'
      ) {
        return info;
      }
      await new Promise((resolve) => setTimeout(resolve, 1));
    }
    return svc.getTask(taskId);
  }

  function serviceWithAppendCounter(): {
    svc: IAgentTaskService;
    persistedChars: () => number;
  } {
    let persistedChars = 0;
    ix.stub(IFileSystemStorageService, {
      read: async () => undefined,
      readStream: async function* () {},
      write: async () => {},
      writeStream: async () => {},
      append: async (_scope: string, _key: string, chunk: Uint8Array) => {
        persistedChars += chunk.byteLength;
      },
      list: async () => [],
      delete: async () => {},
      flush: async () => {},
      close: async () => {},
    });
    return { svc: ix.get(IAgentTaskService), persistedChars: () => persistedChars };
  }

  it('terminates a foreground command that exceeds the output limit and stops forwarding', async () => {
    const svc = ix.get(IAgentTaskService);
    const chunks = Array.from({ length: 20 }, () => 'x'.repeat(MiB));
    const { proc, kill } = streamingProcess(chunks);

    let forwardedChars = 0;
    const onOutput = vi.fn((_kind: 'stdout' | 'stderr', text: string) => {
      forwardedChars += text.length;
    });

    const taskId = svc.registerTask(
      new ProcessTask(proc, 'b3sum --length 18446744073709551615', 'hash', onOutput),
      { detached: false, signal: new AbortController().signal, timeoutMs: 60_000 },
    );

    const info = await waitForTerminal(svc, taskId);

    expect(info?.status).toBe('killed');
    expect(info?.stopReason ?? '').toMatch(/output limit/i);
    expect(kill).toHaveBeenCalledWith('SIGTERM');
    expect(forwardedChars).toBeLessThanOrEqual(LIMIT_BYTES);
  });

  it('also terminates a detached (background) task for the same output', async () => {
    const svc = ix.get(IAgentTaskService);
    const chunks = Array.from({ length: 20 }, () => 'x'.repeat(MiB));
    const { proc, kill } = streamingProcess(chunks);

    const taskId = svc.registerTask(new ProcessTask(proc, 'producer', 'bg'), {
      detached: true,
      timeoutMs: 60_000,
    });

    const info = await waitForTerminal(svc, taskId);

    expect(info?.status).toBe('killed');
    expect(info?.stopReason ?? '').toMatch(/output limit/i);
    expect(kill).toHaveBeenCalledWith('SIGTERM');
  });

  it('stops enqueuing output to disk once the foreground cap trips', async () => {
    const { svc, persistedChars } = serviceWithAppendCounter();

    const chunks = Array.from({ length: 20 }, () => 'x'.repeat(MiB));
    const { proc } = sigtermIgnoringProcess(chunks);

    const taskId = svc.registerTask(new ProcessTask(proc, 'runaway', 'hash', () => {}), {
      detached: false,
      signal: new AbortController().signal,
      timeoutMs: 60_000,
    });

    const info = await waitForTerminal(svc, taskId);

    expect(info?.status).toBe('killed');
    expect(persistedChars()).toBeLessThanOrEqual(17 * MiB);
  });

  it('stops appending persisted output once the output limit trips for a detached process task', async () => {
    const { svc, persistedChars } = serviceWithAppendCounter();

    const chunks = Array.from({ length: 20 }, () => 'x'.repeat(MiB));
    const { proc } = sigtermIgnoringProcess(chunks);

    const taskId = svc.registerTask(new ProcessTask(proc, 'runaway', 'bg', () => {}), {
      detached: true,
      timeoutMs: 60_000,
    });

    const info = await waitForTerminal(svc, taskId);
    await svc.getOutputSnapshot(taskId, 1);

    expect(info?.status).toBe('killed');
    expect(persistedChars()).toBeLessThanOrEqual(17 * MiB);
  });

  it('does not cap or drop a detached subagent result larger than the limit', async () => {
    const { svc, persistedChars } = serviceWithAppendCounter();

    const bigResult = 'y'.repeat(20 * MiB);
    const taskId = svc.registerTask(agentLikeTask(bigResult, 'big subagent result'), {
      detached: true,
      timeoutMs: 60_000,
    });

    const info = await waitForTerminal(svc, taskId);

    expect(info?.status).toBe('completed');
    expect(persistedChars()).toBeGreaterThanOrEqual(bigResult.length);
  });
});

describe('Agent task notification XML', () => {
  it('renders task notifications with escaped attributes and generic children', () => {
    const text = renderNotificationXml({
      id: 'n_"1&2',
      category: 'task',
      type: 'task.done',
      source_kind: 'background_task',
      source_id: 'bg&1',
      title: 'Task finished',
      severity: 'info',
      body: 'The task completed.',
      children: [
        [
          '<output-file path="/tmp/logs/a&amp;b/output.log" bytes="1234">',
          'Read the output file to retrieve the result: /tmp/logs/a&amp;b/output.log',
          '</output-file>',
        ].join('\n'),
      ],
    });

    expect(text).toContain('id="n_&quot;1&amp;2"');
    expect(text).toContain('source_id="bg&amp;1"');
    expect(text).toContain('Title: Task finished');
    expect(text).toContain('Severity: info');
    expect(text).toContain('<output-file path="/tmp/logs/a&amp;b/output.log" bytes="1234">');
    expect(text).toContain(
      'Read the output file to retrieve the result: /tmp/logs/a&amp;b/output.log',
    );
    expect(text).not.toContain('<task-notification>');
    expect(text.trimEnd()).toMatch(/<\/notification>$/);
  });

  it('renders an agent_id attribute when the notification carries one', () => {
    const text = renderNotificationXml({
      id: 'n_lost1',
      category: 'task',
      type: 'task.lost',
      source_kind: 'background_task',
      source_id: 'agent-w7gq3wwj',
      agent_id: 'agent-0',
      title: 'Background agent lost',
      severity: 'warning',
      body: 'Background agent 1 lost.',
    });

    expect(text).toContain('source_id="agent-w7gq3wwj"');
    expect(text).toContain('agent_id="agent-0"');
  });

  it('omits the agent_id attribute when the notification does not carry one', () => {
    const text = renderNotificationXml({
      id: 'n_bash',
      category: 'task',
      type: 'task.completed',
      source_kind: 'background_task',
      source_id: 'bash-abcdef00',
      title: 'Background task completed',
      severity: 'info',
      body: 'echo done completed.',
    });

    expect(text).not.toContain('agent_id=');
  });

  it('ignores unrelated fields while applying attribute fallbacks', () => {
    const text = renderNotificationXml({
      id: '',
      source_kind: 'host',
      tail_output: 'should stay out of the XML',
    });

    expect(text).toContain('id="unknown"');
    expect(text).toContain('category="unknown"');
    expect(text).not.toContain('<task-notification>');
    expect(text).not.toContain('should stay out of the XML');
  });
});
