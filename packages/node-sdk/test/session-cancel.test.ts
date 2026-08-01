import { afterEach, describe, expect, it } from "vitest";

import { createKimiHarness, type KimiError, type Event } from "#/index";

import {
  createTestProviderRuntime,
  makeTempDir,
  removeTempDirs,
  waitForSDKEvent,
} from "./session-runtime-helpers";
import { TEST_IDENTITY } from "./test-identity";

const tempDirs: string[] = [];

afterEach(async () => {
  await removeTempDirs(tempDirs);
});

describe("Session.cancel", () => {
  it("cancels an active streaming turn and emits turn_ended(cancelled)", async () => {
    const homeDir = await makeTempDir(tempDirs, "kimi-sdk-cancel-home-");
    const workDir = await makeTempDir(tempDirs, "kimi-sdk-cancel-work-");
    const harness = createKimiHarness({
      homeDir,
      identity: TEST_IDENTITY,
      providerRuntime: createCancelRuntime(),
    });

    try {
      await harness.setConfig({
        defaultProvider: "kimi-coding",
        defaultModel: "kimi-for-coding",
      });
      const session = await harness.createSession({ id: "ses_cancel_active_turn", workDir });
      const events: Event[] = [];
      const unsubscribe = session.onEvent((event) => {
        events.push(event);
      });
      const started = waitForSDKEvent(session, (event) => event.type === "turn.started");
      const ended = waitForSDKEvent(session, (event) => event.type === "turn.ended");

      await session.prompt("start a turn that will be cancelled");
      const startedEvent = await started;
      await session.cancel();
      const endedEvent = await ended;
      unsubscribe();

      expect(startedEvent).toMatchObject({
        type: "turn.started",
        sessionId: session.id,
      });
      expect(endedEvent).toMatchObject({
        type: "turn.ended",
        sessionId: session.id,
        turnId: startedEvent.type === "turn.started" ? startedEvent.turnId : undefined,
        reason: "cancelled",
      });
      expect(events).toContainEqual(expect.objectContaining({ type: "turn.started" }));
      expect(events).toContainEqual(expect.objectContaining({ type: "turn.ended" }));
    } finally {
      await harness.close();
    }
  });

  it("rejects manual compaction on an empty session with compaction.unable", async () => {
    const homeDir = await makeTempDir(tempDirs, "kimi-sdk-cancel-compact-home-");
    const workDir = await makeTempDir(tempDirs, "kimi-sdk-cancel-compact-work-");
    const harness = createKimiHarness({
      homeDir,
      identity: TEST_IDENTITY,
      providerRuntime: createCancelRuntime(),
    });

    try {
      await harness.setConfig({
        defaultProvider: "kimi-coding",
        defaultModel: "kimi-for-coding",
      });
      const session = await harness.createSession({ id: "ses_cancel_compaction", workDir });

      await expect(
        session.compact({ instruction: "Keep the compact test pending." }),
      ).rejects.toMatchObject({
        name: "KimiError",
        code: "compaction.unable",
      } satisfies Partial<KimiError>);
    } finally {
      await harness.close();
    }
  });

  it("rejects after the session is closed", async () => {
    const homeDir = await makeTempDir(tempDirs, "kimi-sdk-cancel-home-");
    const workDir = await makeTempDir(tempDirs, "kimi-sdk-cancel-work-");
    const harness = createKimiHarness({ homeDir, identity: TEST_IDENTITY });

    try {
      const session = await harness.createSession({ id: "ses_cancel_closed", workDir });
      await session.close();

      await expect(session.cancel()).rejects.toMatchObject({
        name: "KimiError",
        code: "session.closed",
      } satisfies Partial<KimiError>);
      await expect(session.cancelCompaction()).rejects.toMatchObject({
        name: "KimiError",
        code: "session.closed",
      } satisfies Partial<KimiError>);
    } finally {
      await harness.close();
    }
  });
});

describe("KimiHarness.forkSession", () => {
  it("forks a crash-consistent prefix while the source session has an active turn", async () => {
    const homeDir = await makeTempDir(tempDirs, "kimi-sdk-fork-active-home-");
    const workDir = await makeTempDir(tempDirs, "kimi-sdk-fork-active-work-");
    const harness = createKimiHarness({
      homeDir,
      identity: TEST_IDENTITY,
      providerRuntime: createCancelRuntime(),
    });

    try {
      await harness.setConfig({
        defaultProvider: "kimi-coding",
        defaultModel: "kimi-for-coding",
      });
      const session = await harness.createSession({ id: "ses_fork_active_turn", workDir });
      const started = waitForSDKEvent(session, (event) => event.type === "turn.started");
      const ended = waitForSDKEvent(session, (event) => event.type === "turn.ended");

      await session.prompt("keep this turn active");
      await started;
      try {
        const fork = await harness.forkSession({
          id: session.id,
          forkId: "ses_fork_active_child",
        });
        expect(fork.id).toBe("ses_fork_active_child");
        await fork.close();
      } finally {
        await session.cancel().catch(() => undefined);
        await ended.catch(() => undefined);
      }
    } finally {
      await harness.close();
    }
  });
});

function createCancelRuntime() {
  return createTestProviderRuntime({
    // eslint-disable-next-line require-yield -- cancellation rejects before a stream event exists
    stream: async function* (_model, _context, _auth, options) {
      await waitForAbort(options?.signal);
      throwAbortError();
    },
  });
}

function waitForAbort(signal: AbortSignal | undefined): Promise<void> {
  if (signal?.aborted === true) {
    return Promise.resolve();
  }
  return new Promise((resolve) => {
    signal?.addEventListener(
      "abort",
      () => {
        resolve();
      },
      { once: true },
    );
  });
}

function throwAbortError(): never {
  throw new DOMException("The operation was aborted.", "AbortError");
}
