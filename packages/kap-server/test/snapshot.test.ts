/**
 * `GET /api/v1/sessions/{session_id}/snapshot` — atomic-at-a-watermark
 * snapshot shape and watermark consistency.
 */

import { mkdir, mkdtemp, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

import {
  type DomainEvent,
  IEventBus,
  IAgentLifecycleService,
  ILogService,
  ISessionContext,
  ISessionLifecycleService,
} from '@moonshot-ai/agent-core-v2';
import { sessionSnapshotResponseSchema } from '../src/protocol/rest-snapshot';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';

import { registerSnapshotRoutes } from '../src/routes/snapshot';
import { SnapshotNotFoundError } from '../src/services/snapshot';
import { type RunningServer, startServer } from '../src/start';
import { authHeaders } from './helpers/auth';

function fakeAccessor(entries: ReadonlyArray<readonly [unknown, unknown]>) {
  const services = new Map<unknown, unknown>(entries);
  return {
    get<T>(id: unknown): T {
      if (!services.has(id)) {
        throw new Error(`unexpected service request: ${String(id)}`);
      }
      return services.get(id) as T;
    },
  };
}

describe('server-v2 snapshot route error mapping', () => {
  function captureHandler(
    deps: { core: unknown; broadcaster: unknown; reader: unknown },
    env?: Record<string, string>,
  ) {
    const previous = new Map<string, string | undefined>();
    for (const [k, v] of Object.entries(env ?? {})) {
      previous.set(k, process.env[k]);
      process.env[k] = v;
    }
    let handler:
      | ((
          req: { id: string; params: { session_id: string } },
          reply: { send(payload: unknown): unknown },
        ) => Promise<void> | void)
      | undefined;
    try {
      registerSnapshotRoutes(
        {
          get: (_path, _options, h) => {
            handler = h;
          },
        },
        deps as never,
      );
    } finally {
      for (const [k, v] of previous) {
        if (v === undefined) delete process.env[k];
        else process.env[k] = v;
      }
    }
    return handler!;
  }

  it('maps SnapshotNotFoundError to 40401', async () => {
    const warns: unknown[] = [];
    const core = {
      accessor: fakeAccessor([[ILogService, { warn: (...a: unknown[]) => warns.push(a) }]]),
    };
    const reader = {
      read: async () => {
        throw new SnapshotNotFoundError('sess_missing');
      },
    };
    const handler = captureHandler({ core, broadcaster: {}, reader });
    let payload: unknown;
    await handler(
      { id: 'req_404', params: { session_id: 'sess_missing' } },
      { send: (v) => (payload = v) },
    );
    expect((payload as { code: number }).code).toBe(40401);
    expect(warns).toHaveLength(0);
  });

  it('maps SnapshotTimeoutError to 50001 and logs snapshot.timeout', async () => {
    const warns: unknown[] = [];
    const core = {
      accessor: fakeAccessor([[ILogService, { warn: (...a: unknown[]) => warns.push(a) }]]),
    };
    const reader = {
      read: () => new Promise<never>(() => {}), // hangs → triggers the timeout race
    };
    const handler = captureHandler(
      { core, broadcaster: {}, reader },
      { KIMI_SNAPSHOT_TIMEOUT_MS: '150' },
    );
    let payload: unknown;
    await handler(
      { id: 'req_to', params: { session_id: 'sess_slow' } },
      { send: (v) => (payload = v) },
    );
    expect((payload as { code: number }).code).toBe(50001);
    expect(warns).toHaveLength(1);
    expect((warns[0] as unknown[])[0]).toBe('snapshot.timeout');
  });
});

describe('server-v2 GET /api/v1/sessions/:id/snapshot', () => {
  let server: RunningServer | undefined;
  let home: string | undefined;
  let base: string;

  beforeEach(async () => {
    home = await mkdtemp(join(tmpdir(), 'kimi-snapshot-test-'));
    server = await startServer({ host: '127.0.0.1', port: 0, homeDir: home, logLevel: 'silent' });
    base = `http://127.0.0.1:${server.port}`;
  });

  afterEach(async () => {
    if (server !== undefined) {
      await server.close();
      server = undefined;
    }
    if (home !== undefined) {
      await rm(home, { recursive: true, force: true });
      home = undefined;
    }
  });

  async function createSession(): Promise<string> {
    const res = await fetch(`${base}/api/v1/sessions`, {
      method: 'POST',
      headers: authHeaders(server as RunningServer, { 'content-type': 'application/json' }),
      body: JSON.stringify({ metadata: { cwd: home } }),
    } as never);
    const body = (await res.json()) as { code: number; data: { id: string } };
    expect(body.code).toBe(0);
    return body.data.id;
  }

  async function ensureMainAgent(sessionId: string): Promise<void> {
    const session = server!.core.accessor.get(ISessionLifecycleService).get(sessionId);
    const agents = session!.accessor.get(IAgentLifecycleService);
    if (agents.get('main') === undefined) await agents.create({ agentId: 'main' });
  }

  function emit(sessionId: string, event: DomainEvent): void {
    const session = server!.core.accessor.get(ISessionLifecycleService).get(sessionId);
    const main = session!.accessor.get(IAgentLifecycleService).get('main');
    main!.accessor.get(IEventBus).publish(event);
  }

  async function snapshot(sid: string) {
    const res = await fetch(`${base}/api/v1/sessions/${sid}/snapshot`, {
      headers: authHeaders(server as RunningServer),
    } as never);
    const body = (await res.json()) as { code: number; data: unknown };
    expect(body.code).toBe(0);
    return sessionSnapshotResponseSchema.parse(body.data);
  }

  it('returns a well-formed snapshot for a fresh session', async () => {
    const sid = await createSession();
    const snap = await snapshot(sid);

    expect(snap.session.id).toBe(sid);
    expect(snap.as_of_seq).toBe(1);
    expect(snap.epoch).toMatch(/^ep_/);
    expect(snap.messages.items).toEqual([]);
    expect(snap.in_flight_turn).toBeNull();
    expect(snap.pending_approvals).toEqual([]);
    expect(snap.pending_questions).toEqual([]);
  });

  it('reflects the durable watermark and in-flight turn after events', async () => {
    const sid = await createSession();
    await ensureMainAgent(sid);
    await snapshot(sid); // activate the journal after agent metadata records

    emit(sid, {
      type: 'turn.started',
      turnId: 1,
    } as unknown as DomainEvent); // durable → seq 1
    emit(sid, { type: 'assistant.delta', turnId: 1, delta: 'Hello' } as unknown as DomainEvent); // volatile

    const snap = await snapshot(sid);
    expect(snap.as_of_seq).toBeGreaterThanOrEqual(2);
    expect(snap.in_flight_turn).toMatchObject({
      turn_id: 1,
      assistant_text: 'Hello',
    });
  });

  it('returns 404 for an unknown session', async () => {
    const res = await fetch(`${base}/api/v1/sessions/sess_does_not_exist/snapshot`, {
      headers: authHeaders(server as RunningServer),
    } as never);
    const body = (await res.json()) as { code: number };
    expect(body.code).not.toBe(0);
  });

  // A session that exists on disk but is not live in this process must load
  // from disk instead of returning 40401.
  it('loads a cold (not live) session from disk instead of 404', async () => {
    const sid = await createSession();

    await server!.close();
    server = undefined;
    server = await startServer({ host: '127.0.0.1', port: 0, homeDir: home, logLevel: 'silent' });
    base = `http://127.0.0.1:${server.port}`;

    // Guard: nothing is live in the new process — the session is cold.
    expect(server!.core.accessor.get(ISessionLifecycleService).get(sid)).toBeUndefined();

    const snap = await snapshot(sid);
    expect(snap.session.id).toBe(sid);
  });

  // The auto reader must source messages from `agents/main/wire.jsonl` on disk
  // — not from a live (resumed) context. We seed a wire log, restart so the
  // session is genuinely cold, then assert the snapshot returns the on-disk
  // transcript while the scope stays un-materialized.
  it('auto reader returns messages read directly from wire.jsonl for a cold session', async () => {
    const sid = await createSession();
    const live = server!.core.accessor.get(ISessionLifecycleService).get(sid);
    if (live === undefined) throw new Error(`session ${sid} not found`);
    const metaScope = live.accessor.get(ISessionContext).metaScope;

    const wireDir = join(home as string, metaScope, 'agents', 'main');
    await mkdir(wireDir, { recursive: true });
    const records = [
      { type: 'metadata', protocol_version: '1.4', created_at: Date.now() },
      {
        type: 'context.append_message',
        message: { role: 'user', content: [{ type: 'text', text: 'hello-from-disk' }], toolCalls: [] },
      },
      {
        type: 'context.append_message',
        message: {
          role: 'assistant',
          content: [{ type: 'text', text: 'hi-from-disk' }],
          toolCalls: [],
        },
      },
    ];
    await writeFile(
      join(wireDir, 'wire.jsonl'),
      records.map((r) => JSON.stringify(r)).join('\n') + '\n',
      'utf-8',
    );

    await server!.close();
    server = undefined;
    server = await startServer({ host: '127.0.0.1', port: 0, homeDir: home, logLevel: 'silent' });
    base = `http://127.0.0.1:${server.port}`;

    // Guard: still cold — the auto reader must serve from disk, not resume.
    expect(server!.core.accessor.get(ISessionLifecycleService).get(sid)).toBeUndefined();

    const snap = await snapshot(sid);
    expect(snap.session.id).toBe(sid);
    expect(snap.messages.items).toHaveLength(2);
    expect((snap.messages.items[0]!.content[0] as { text: string }).text).toBe('hello-from-disk');
    expect((snap.messages.items[1]!.content[0] as { text: string }).text).toBe('hi-from-disk');
    expect(snap.epoch).toMatch(/^ep_/);
  });

});
