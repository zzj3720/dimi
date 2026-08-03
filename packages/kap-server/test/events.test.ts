/**
 * `GET /sessions/:session_id/events` — SSE route: live-only subscription,
 * `?event_seq=` replay from the broadcaster's journal/tail, resync signaling,
 * and validation. Exercised through a fake broadcaster (the route only depends
 * on `subscribe` / `unsubscribe` / `getBufferedSince`), same capture-handler
 * pattern as `snapshot.test.ts`.
 */

import { describe, expect, it } from 'vitest';

import { registerEventsRoute } from '../src/routes/events';
import type {
  BroadcastTarget,
  BufferedSinceResult,
  SessionEventBroadcaster,
} from '../src/transport/ws/v1/sessionEventBroadcaster';
import type { EventEnvelope } from '../src/transport/ws/v1/sessionEventJournal';

function envelope(seq: number, type = 'turn.started'): EventEnvelope {
  return {
    type,
    seq,
    epoch: 'ep_1',
    session_id: 's1',
    timestamp: '2026-01-01T00:00:00.000Z',
    payload: { type },
  };
}

function frame(envelope: EventEnvelope): string {
  return `event: ${envelope.type}\ndata: ${JSON.stringify(envelope)}\n\n`;
}

class FakeBroadcaster {
  /** Call order: 'subscribe' | 'replay' | 'unsubscribe'. */
  readonly calls: string[] = [];
  readonly subscribeCalls: string[] = [];
  readonly unsubscribeCalls: string[] = [];
  readonly replayCalls: Array<{ sessionId: string; seq: number }> = [];
  target: BroadcastTarget | undefined;
  subscribeResult = true;
  replayResult: BufferedSinceResult | undefined;

  async subscribe(sessionId: string, target: BroadcastTarget): Promise<boolean> {
    this.calls.push('subscribe');
    this.subscribeCalls.push(sessionId);
    this.target = target;
    return this.subscribeResult;
  }

  unsubscribe(sessionId: string, _target: BroadcastTarget): void {
    this.calls.push('unsubscribe');
    this.unsubscribeCalls.push(sessionId);
  }

  async getBufferedSince(
    sessionId: string,
    cursor: { seq: number },
  ): Promise<BufferedSinceResult> {
    this.calls.push('replay');
    this.replayCalls.push({ sessionId, seq: cursor.seq });
    return (
      this.replayResult ?? {
        events: [],
        resyncRequired: false,
        currentSeq: cursor.seq,
        epoch: 'ep_1',
      }
    );
  }
}

class FakeRawResponse {
  readonly chunks: string[] = [];
  ended = false;

  writeHead(): void {}

  write(chunk: string): void {
    this.chunks.push(chunk);
  }

  end(): void {
    this.ended = true;
  }

  get body(): string {
    return this.chunks.join('');
  }
}

class FakeReq {
  readonly id = 'req_events_1';
  readonly params: { session_id: string };
  readonly query: Record<string, unknown>;
  readonly raw: { on(event: string, cb: () => void): void };
  private readonly closeHandlers: Array<() => void> = [];

  constructor(sessionId: string, query: Record<string, unknown> = {}) {
    this.params = { session_id: sessionId };
    this.query = query;
    this.raw = {
      on: (event, cb) => {
        if (event === 'close') this.closeHandlers.push(cb);
      },
    };
  }

  /** Simulate the request being torn down; clears the heartbeat interval. */
  close(): void {
    for (const cb of [...this.closeHandlers]) cb();
  }
}

interface RunOptions {
  sessionId?: string;
  query?: Record<string, unknown>;
  subscribeResult?: boolean;
  replayResult?: BufferedSinceResult;
}

interface RunResult {
  broadcaster: FakeBroadcaster;
  raw: FakeRawResponse;
  req: FakeReq;
  json: Record<string, unknown> | null;
}

type CapturedHandler = (req: unknown, reply: unknown) => Promise<void> | void;

function captureHandler(broadcaster: FakeBroadcaster): CapturedHandler {
  let handler: CapturedHandler | undefined;
  registerEventsRoute(
    {
      get: (_path, _options, h) => {
        handler = h as unknown as CapturedHandler;
      },
    },
    {
      broadcaster: broadcaster as unknown as SessionEventBroadcaster,
      core: { accessor: { get: () => ({ resume: async () => ({}) }) } } as never,
    },
  );
  if (handler === undefined) throw new Error('events route handler not captured');
  return handler;
}

async function run(options: RunOptions = {}): Promise<RunResult> {
  const broadcaster = new FakeBroadcaster();
  if (options.subscribeResult !== undefined) broadcaster.subscribeResult = options.subscribeResult;
  if (options.replayResult !== undefined) broadcaster.replayResult = options.replayResult;

  const raw = new FakeRawResponse();
  let json: Record<string, unknown> | null = null;
  const reply = {
    hijack: () => {},
    code: () => reply,
    send: (payload: unknown) => {
      json = payload as Record<string, unknown>;
    },
    raw,
  };
  const req = new FakeReq(options.sessionId ?? 's1', options.query ?? {});
  await captureHandler(broadcaster)(req, reply);
  return { broadcaster, raw, req, json };
}

describe('GET /sessions/:session_id/events (SSE)', () => {
  it('streams live only when event_seq is absent', async () => {
    const { broadcaster, raw, req } = await run();

    expect(broadcaster.calls).toEqual(['subscribe']);
    expect(broadcaster.replayCalls).toEqual([]);
    expect(raw.body).toBe(': connected\n\n');

    // Live fan-out writes the envelope frame once subscribed.
    broadcaster.target?.send(envelope(1));
    expect(raw.body).toBe(`: connected\n\n${frame(envelope(1))}`);

    req.close();
    expect(broadcaster.calls).toEqual(['subscribe', 'unsubscribe']);
  });

  it('subscribes before replaying, then replays buffered events after event_seq', async () => {
    const { broadcaster, raw, req } = await run({
      query: { event_seq: '2' },
      replayResult: {
        events: [
          { seq: 3, envelope: envelope(3) },
          { seq: 4, envelope: envelope(4, 'turn.ended') },
        ],
        resyncRequired: false,
        currentSeq: 4,
        epoch: 'ep_1',
      },
    });

    // WS-consistent ordering: the target is subscribed before the replay read
    // so no event dispatched between the read and the live fan-out is lost.
    expect(broadcaster.calls).toEqual(['subscribe', 'replay']);
    expect(broadcaster.replayCalls).toEqual([{ sessionId: 's1', seq: 2 }]);
    expect(raw.body).toBe(`: connected\n\n${frame(envelope(3))}${frame(envelope(4, 'turn.ended'))}`);

    // The stream stays live after the replay.
    broadcaster.target?.send(envelope(5));
    expect(raw.body).toContain('"seq":5');

    req.close();
    expect(broadcaster.unsubscribeCalls).toEqual(['s1']);
  });

  it('emits a WS-shaped resync_required frame when the cursor cannot be replayed', async () => {
    const { broadcaster, raw, req } = await run({
      query: { event_seq: '1' },
      replayResult: {
        events: [],
        resyncRequired: 'buffer_overflow',
        currentSeq: 10,
        epoch: 'ep_1',
      },
    });

    expect(raw.body).toBe(
      `: connected\n\n` +
        `event: resync_required\ndata: ${JSON.stringify({
          type: 'resync_required',
          session_id: 's1',
          reason: 'buffer_overflow',
          current_seq: 10,
          epoch: 'ep_1',
        })}\n\n`,
    );

    // Resync is not fatal: the subscription stays live for future events.
    expect(broadcaster.subscribeCalls).toEqual(['s1']);
    req.close();
  });

  it.each(['abc', '-1', '1.5', ''])('rejects malformed event_seq %j with 400', async (value) => {
    const { broadcaster, raw, json, req } = await run({ query: { event_seq: value } });

    expect(json).toMatchObject({
      code: 40000,
      msg: 'event_seq must be a non-negative integer',
    });
    expect(raw.body).toBe(''); // rejected before hijack
    expect(broadcaster.subscribeCalls).toEqual([]);
    req.close();
  });

  it('emits an error frame and ends when the session is unknown', async () => {
    const { raw, req } = await run({ subscribeResult: false });

    expect(raw.body).toBe(
      ': connected\n\n' + 'event: error\ndata: {"code":40401,"msg":"session not found"}\n\n',
    );
    expect(raw.ended).toBe(true);
    req.close();
  });
});
