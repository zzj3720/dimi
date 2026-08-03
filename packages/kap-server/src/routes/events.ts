/**
 * `GET /sessions/:session_id/events` — Server-Sent Events (SSE) stream for a
 * session's live events.
 *
 * The SSE transport is the client-friendly projection of the v1 WS event
 * stream: the Native SDK client has no WebSocket support, but consumes
 * `text/event-stream` natively (fetch `.stream`). Every frame is the
 * broadcaster's `EventEnvelope` (`{type, seq, timestamp, payload, ...}`)
 * serialized as JSON:
 *
 *   event: <envelope.type>
 *   data: <JSON envelope>
 *
 * Subscription semantics are the PoC minimal set: one connection = one
 * session's per-agent event stream (the broadcaster's session-scoped fan-out,
 * including transcript ops when the client asks for transcript grades). Global
 * events (`event.session.*` / `event.workspace.*` / `event.config.*`) ARE
 * delivered too: `SessionEventBroadcaster` fans them out to every registered
 * target (verified live: a `session.meta.updated` for this session arrives on
 * the SSE stream), so a client that only subscribes to one session still sees
 * that session's global events. Clients wanting other sessions' global events
 * poll REST or subscribe per session.
 *
 * Replay: the optional `?event_seq=<seq>` query parameter asks the server to
 * replay the buffered durable events with `seq > event_seq` (from the
 * broadcaster's journal / in-memory tail via `getBufferedSince`) before live
 * delivery — the same catch-up the WS transport runs on subscribe with a
 * cursor. Replay frames use the identical `event: <type>\ndata: <JSON
 * envelope>` shape as live frames, so a client that tracks the last-applied
 * `seq` can gap-fill without reconnecting. When the cursor cannot be replayed
 * (gap overflow, epoch change, stale cursor) the server emits a WS-shaped
 * `resync_required` frame — `{session_id, reason, current_seq, epoch}` — and
 * keeps the stream live; the client should rebuild from the snapshot and
 * reconnect with the new cursor. Absent `event_seq`, the stream is live-only.
 *
 * Heartbeats (`: ping` comment frames, 15s) keep proxies from closing idle
 * connections; the client treats a comment-only line as a keepalive.
 */

import type { FastifyReply, FastifyRequest } from 'fastify';

import { ISessionLifecycleService, type Scope } from '@dimi-agent/agent-core-v2';

import type {
  BroadcastTarget,
  SessionEventBroadcaster,
} from '../transport/ws/v1/sessionEventBroadcaster';
import type { EventEnvelope } from '../transport/ws/v1/sessionEventJournal';

const SSE_HEADERS = {
  'Content-Type': 'text/event-stream',
  'Cache-Control': 'no-cache',
  Connection: 'keep-alive',
  'X-Accel-Buffering': 'no',
} as const;

const HEARTBEAT_INTERVAL_MS = 15_000;

export interface EventsRouteOptions {
  readonly broadcaster: SessionEventBroadcaster;
  /** Core scope access for session activation (resume) before subscribing. */
  readonly core: Scope;
}

interface RouteHost {
  get(
    path: string,
    options: { schema?: Record<string, unknown> },
    handler: (req: FastifyRequest, reply: FastifyReply) => Promise<void> | void,
  ): unknown;
}

export function registerEventsRoute(app: RouteHost, opts: EventsRouteOptions): void {
  app.get(
    '/sessions/:session_id/events',
    {
      schema: {
        params: {
          type: 'object',
          properties: { session_id: { type: 'string' } },
          required: ['session_id'],
        },
        querystring: {
          type: 'object',
          properties: { event_seq: { type: 'string' } },
          required: [],
        },
      },
    },
    async (req, reply) => {
      const { session_id } = req.params as { session_id: string };
      if (!session_id) {
        await reply.code(400).send({
          code: 40000,
          msg: 'session_id required',
          data: null,
          request_id: req.id,
        });
        return;
      }

      // Optional replay cursor: `?event_seq=<seq>` replays the buffered durable
      // events after that seq before live delivery.
      const eventSeq = parseEventSeq((req.query as { event_seq?: unknown }).event_seq);
      if (eventSeq !== undefined && (!Number.isInteger(eventSeq) || eventSeq < 0)) {
        await reply.code(400).send({
          code: 40000,
          msg: 'event_seq must be a non-negative integer',
          data: null,
          request_id: req.id,
        });
        return;
      }

      // Take over the raw response for the SSE stream.
      reply.hijack();
      const raw = reply.raw;
      raw.writeHead(200, SSE_HEADERS);
      raw.write(': connected\n\n');

      let closed = false;
      const target: BroadcastTarget = {
        send(envelope: EventEnvelope): void {
          if (closed) return;
          raw.write(`event: ${envelope.type}\ndata: ${JSON.stringify(envelope)}\n\n`);
        },
      };

      const subscribed = await opts.broadcaster.subscribe(session_id, target);
      if (!subscribed) {
        // The session may exist on disk but be cold (not activated): the
        // lifecycle service only returns live sessions, so resume it first
        // (the same activation the REST profile route performs), then retry.
        const resumed = await opts.core.accessor.get(ISessionLifecycleService).resume(session_id);
        if (resumed === undefined) {
          closed = true;
          raw.write('event: error\ndata: {"code":40401,"msg":"session not found"}\n\n');
          raw.end();
          return;
        }
        const retry = await opts.broadcaster.subscribe(session_id, target);
        if (!retry) {
          closed = true;
          raw.write('event: error\ndata: {"code":40401,"msg":"session not found"}\n\n');
          raw.end();
          return;
        }
      }

      // Subscribe before replay (like the WS attach path): registering the
      // target first means no event dispatched between the replay read and the
      // live fan-out can be lost — a narrow race may double-deliver instead,
      // which the client dedups by envelope seq.
      if (eventSeq !== undefined) {
        const result = await opts.broadcaster.getBufferedSince(session_id, { seq: eventSeq });
        if (result.resyncRequired !== false) {
          // Mirror the WS `resync_required` payload so the client can rebuild
          // from the snapshot and reconnect with the reported cursor.
          raw.write(
            `event: resync_required\ndata: ${JSON.stringify({
              type: 'resync_required',
              session_id,
              reason: result.resyncRequired,
              current_seq: result.currentSeq,
              epoch: result.epoch,
            })}\n\n`,
          );
        } else {
          for (const { envelope } of result.events) target.send(envelope);
        }
      }

      const heartbeat = setInterval(() => {
        if (!closed) raw.write(': ping\n\n');
      }, HEARTBEAT_INTERVAL_MS);

      req.raw.on('close', () => {
        closed = true;
        clearInterval(heartbeat);
        void opts.broadcaster.unsubscribe(session_id, target);
      });
    },
  );
}

/**
 * Parse the optional `?event_seq=` replay cursor. `undefined` = absent
 * (live-only stream); `NaN` = present but malformed — the caller rejects it
 * with a 400. A valid cursor is a non-negative integer (the journal seq
 * domain, matching `sessionCursorSchema.seq`).
 */
function parseEventSeq(raw: unknown): number | undefined {
  if (raw === undefined) return undefined;
  if (typeof raw !== 'string' || !/^\d+$/.test(raw)) return Number.NaN;
  return Number(raw);
}
