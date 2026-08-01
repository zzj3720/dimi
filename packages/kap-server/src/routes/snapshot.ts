/**
 * `GET /sessions/{session_id}/snapshot` — IM-style initial sync.
 *
 * Delegates to `ISnapshotReader`, which reads `state.json` and
 * `agents/main/wire.jsonl` directly without materializing the session scope.
 *
 * **Timeout**: the auto path races against a hard `DIMI_SNAPSHOT_TIMEOUT_MS`
 * ceiling (default 4000ms, under traefik's 5s cut-off). Timeout returns 50001
 * with a structured `snapshot.timeout` log line so the gateway never sees a 499.
 *
 * **Error mapping**: `SnapshotNotFoundError` → 40401; `SnapshotTimeoutError` →
 * 50001; everything else falls through to the global error handler (→ 50001).
 */

import { ILogService, type Scope } from '@moonshot-ai/agent-core-v2';
import { ErrorCode } from '../protocol/error-codes';
import { sessionSnapshotResponseSchema, type SessionSnapshotResponse } from '../protocol/rest-snapshot';
import { z } from 'zod';

import { errEnvelope, okEnvelope } from '../envelope';
import { defineRoute } from '../middleware/defineRoute';
import {
  SnapshotNotFoundError,
  SnapshotTimeoutError,
  loadSnapshotConfig,
} from '../services/snapshot';
import type { ISnapshotReader } from '../services/snapshot';

const sessionIdParamSchema = z.object({
  session_id: z.string().min(1),
});

interface SnapshotRouteHost {
  get(
    path: string,
    options: { preHandler: unknown[]; schema?: Record<string, unknown> } | undefined,
    handler: (
      req: { id: string; params: { session_id: string } },
      reply: { send(payload: unknown): unknown },
    ) => Promise<void> | void,
  ): unknown;
}

export interface SnapshotRouteDeps {
  readonly core: Scope;
  readonly reader: ISnapshotReader;
}

export function registerSnapshotRoutes(app: SnapshotRouteHost, deps: SnapshotRouteDeps): void {
  const { core, reader } = deps;
  const config = loadSnapshotConfig();

  const route = defineRoute(
    {
      method: 'GET',
      path: '/sessions/{session_id}/snapshot',
      params: sessionIdParamSchema,
      success: { data: sessionSnapshotResponseSchema },
      errors: {
        [ErrorCode.SESSION_NOT_FOUND]: {},
        [ErrorCode.INTERNAL_ERROR]: {},
      },
      description:
        'Atomic session snapshot for client rebuild: state + as_of_seq watermark + epoch',
      tags: ['sessions'],
    },
    async (req, reply) => {
      const { session_id } = req.params;
      try {
        const data = await readViaReader(reader, session_id, config.timeoutMs);
        reply.send(okEnvelope(data, req.id));
      } catch (err) {
        if (err instanceof SnapshotNotFoundError) {
          reply.send(errEnvelope(ErrorCode.SESSION_NOT_FOUND, err.message, req.id, err.stack));
          return;
        }
        if (err instanceof SnapshotTimeoutError) {
          core.accessor
            .get(ILogService)
            .warn('snapshot.timeout', { sid: session_id, duration_ms: err.timeoutMs });
          reply.send(errEnvelope(ErrorCode.INTERNAL_ERROR, err.message, req.id, err.stack));
          return;
        }
        throw err;
      }
    },
  );
  app.get(route.path, route.options, route.handler as Parameters<SnapshotRouteHost['get']>[2]);
}

async function readViaReader(
  reader: ISnapshotReader,
  sid: string,
  timeoutMs: number,
): Promise<SessionSnapshotResponse> {
  let timer: NodeJS.Timeout | undefined;
  const timeoutPromise = new Promise<never>((_resolve, reject) => {
    timer = setTimeout(() => reject(new SnapshotTimeoutError(sid, timeoutMs)), timeoutMs);
    timer.unref?.();
  });
  try {
    return await Promise.race([reader.read(sid), timeoutPromise]);
  } finally {
    if (timer !== undefined) clearTimeout(timer);
  }
}
