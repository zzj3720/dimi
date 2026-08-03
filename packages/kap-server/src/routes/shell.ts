/**
 * `POST /sessions/{session_id}/shell` — run a user-initiated `!` shell
 * command over REST.
 *
 * The REST shell surface exists for clients that have no IPC channel (e.g.
 * the Zig TUI): the `!` prefix is interpreted by the CLI/TUI, but a client
 * that cannot reach the engine over IPC needs a plain HTTP route to execute
 * the command. It is a thin pass-through to the Agent-scoped
 * `IAgentShellCommandService.run` (the same service the IPC RPC
 * `runShellCommand` resolves): resolve the session (cold-loading it like the
 * other main-agent routes), materialize the main agent, and forward the
 * `RunShellCommandResult` verbatim as the envelope `data`.
 *
 * The result shape mirrors `RunShellCommandResult`:
 * `{ stdout, stderr, isError?, backgrounded? }`.
 */

import { IAgentShellCommandService, ISessionLifecycleService, type Scope } from '@dimi-agent/agent-core-v2';
import { z } from 'zod';

import { errEnvelope, okEnvelope } from '../envelope';
import { defineRoute } from '../middleware/defineRoute';
import { ErrorCode } from '../protocol/error-codes';
import { ensureMainAgent } from '../transport/mainAgent';

interface ShellRouteHost {
  post(
    path: string,
    options: { preHandler: unknown[]; schema?: Record<string, unknown> },
    handler: (
      req: { id: string; body: unknown; params: unknown },
      reply: { send(payload: unknown): unknown },
    ) => Promise<void> | void,
  ): unknown;
}

const sessionIdParamSchema = z.object({
  session_id: z.string().min(1),
});

// Mirrors the IPC `runShellCommand` payload contract (`runShellCommandPayloadSchema`)
// so REST and RPC clients send byte-identical bodies.
const shellCommandRequestSchema = z.object({
  command: z.string(),
  commandId: z.string().optional(),
});

const shellCommandResultSchema = z.object({
  stdout: z.string(),
  stderr: z.string(),
  isError: z.boolean().optional(),
  backgrounded: z.boolean().optional(),
});

const detailsSchema = z.array(z.object({ path: z.string(), message: z.string() }));

export function registerShellRoute(app: ShellRouteHost, core: Scope): void {
  const runShellCommandRoute = defineRoute(
    {
      method: 'POST',
      path: '/sessions/{session_id}/shell',
      params: sessionIdParamSchema,
      body: shellCommandRequestSchema,
      success: { data: shellCommandResultSchema },
      errors: {
        [ErrorCode.VALIDATION_FAILED]: { detailsSchema },
        [ErrorCode.SESSION_NOT_FOUND]: {},
      },
      description: 'Run a user-initiated `!` shell command in a session',
      tags: ['sessions'],
    },
    async (req, reply) => {
      const { session_id } = req.params;
      // `resume` (not `get`) so a freshly-opened cold session can execute
      // commands; it returns undefined for an unknown session or a session
      // whose workspace is gone, reported as `session.not_found` (40401).
      const session = await core.accessor.get(ISessionLifecycleService).resume(session_id);
      if (session === undefined) {
        reply.send(
          errEnvelope(ErrorCode.SESSION_NOT_FOUND, `session ${session_id} does not exist`, req.id),
        );
        return;
      }
      const agent = await ensureMainAgent(session);
      const shell = agent.accessor.get(IAgentShellCommandService);
      const result = await shell.run({
        command: req.body.command,
        commandId: req.body.commandId,
      });
      reply.send(okEnvelope(result, req.id));
    },
  );
  app.post(
    runShellCommandRoute.path,
    runShellCommandRoute.options,
    runShellCommandRoute.handler as Parameters<ShellRouteHost['post']>[2],
  );
}
