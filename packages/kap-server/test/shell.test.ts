/**
 * `POST /api/v1/sessions/{session_id}/shell` — REST surface for user-initiated
 * `!` shell commands (no IPC channel needed, e.g. the Zig TUI).
 *
 * Wiring: real kap-server; the command is genuinely executed through the
 * Agent-scoped `IAgentShellCommandService` (the same service the IPC RPC
 * resolves). Exercises the Fastify body/param validation (40001), the
 * session-not-found path (40401), and the `RunShellCommandResult` envelope.
 *
 * Run: `pnpm --filter @dimi-agent/kap-server exec vitest run test/shell.test.ts`.
 */
import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";

import { afterEach, beforeEach, describe, expect, it } from "vitest";

import { type RunningServer, startServer } from "../src/start";
import { authHeaders } from "./helpers/auth";

interface Envelope<T> {
  code: number;
  msg: string;
  data: T;
  request_id: string;
  details?: { path: string; message: string }[];
}

interface ShellResult {
  stdout: string;
  stderr: string;
  isError?: boolean;
  backgrounded?: boolean;
}

describe("server-v2 POST /api/v1/sessions/:session_id/shell", () => {
  let server: RunningServer | undefined;
  let home: string | undefined;
  let base: string;

  beforeEach(async () => {
    home = await mkdtemp(join(tmpdir(), "dimi-server-v2-shell-"));
    server = await startServer({
      host: "127.0.0.1",
      port: 0,
      homeDir: home,
      logLevel: "silent",
    });
    base = `http://127.0.0.1:${server.port}`;
  });

  afterEach(async () => {
    if (server !== undefined) {
      await server.close();
      server = undefined;
    }
    if (home !== undefined) {
      await new Promise((resolve) => setTimeout(resolve, 25));
      await rm(home, { recursive: true, force: true, maxRetries: 5, retryDelay: 50 } as never);
      home = undefined;
    }
  });

  async function createSession(): Promise<string> {
    const res = await fetch(`${base}/api/v1/sessions`, {
      method: "POST",
      headers: authHeaders(server as RunningServer, { "content-type": "application/json" }),
      body: JSON.stringify({ metadata: { cwd: home } }),
    } as never);
    const body = (await res.json()) as Envelope<{ id: string }>;
    expect(body.code).toBe(0);
    return body.data.id;
  }

  async function runShell(
    sessionId: string,
    body: Record<string, unknown>,
  ): Promise<{ status: number; body: Envelope<ShellResult> }> {
    const res = await fetch(`${base}/api/v1/sessions/${sessionId}/shell`, {
      method: "POST",
      headers: authHeaders(server as RunningServer, { "content-type": "application/json" }),
      body: JSON.stringify(body),
    } as never);
    return { status: res.status, body: (await res.json()) as Envelope<ShellResult> };
  }

  it("executes a foreground command and returns the shell result envelope", async () => {
    const sid = await createSession();
    const { status, body } = await runShell(sid, {
      command: "echo shell-route-ok",
      commandId: "cmd_1",
    });

    expect(status).toBe(200);
    expect(body.code).toBe(0);
    expect(body.msg).toBe("success");
    expect(body.data.stdout).toContain("shell-route-ok");
    expect(body.data.stderr).toBe("");
    expect(body.data.isError).not.toBe(true);
  });

  it("reports a failing command as isError=true with the same code:0 envelope", async () => {
    const sid = await createSession();
    const { body } = await runShell(sid, { command: "echo oops >&2; exit 3" });

    expect(body.code).toBe(0);
    expect(body.data.isError).toBe(true);
    expect(body.data.stdout + body.data.stderr).toContain("oops");
  });

  it("rejects a body without a command (40001)", async () => {
    const sid = await createSession();
    const { status, body } = await runShell(sid, {});

    expect(status).toBe(200);
    expect(body.code).toBe(40001);
    expect(body.details?.[0]?.path).toBe("command");
  });

  it("rejects a non-string command (40001)", async () => {
    const sid = await createSession();
    const { body } = await runShell(sid, { command: 42 });

    expect(body.code).toBe(40001);
    expect(body.details?.[0]?.path).toBe("command");
  });

  it("returns 40401 for an unknown session", async () => {
    const { body } = await runShell("sess_missing_shell", { command: "echo x" });

    expect(body.code).toBe(40401);
    expect(body.msg).toMatch(/does not exist/);
  });
});
