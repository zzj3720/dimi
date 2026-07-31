/**
 * Scenario: public session creation, close, and resume behavior.
 * Wiring: real in-process runtime; no provider calls.
 */
import { mkdtemp, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

import type { Kaos } from '@moonshot-ai/kaos';
import { afterEach, describe, expect, it } from 'vitest';

import { createKimiHarness, KimiHarness, type KimiError } from '#/index';
import { SDKRpcClientBase } from '#/rpc';
import type { ResumeSessionInput, ResumedSessionSummary } from '#/types';
import { recordingTelemetry, type TelemetryRecord } from './telemetry';
import { TEST_IDENTITY } from './test-identity';

const tempDirs: string[] = [];
const stdioFixture = join(
  import.meta.dirname,
  '../../agent-core-v2/test/agent/mcp/fixtures/mock-stdio-server.mjs',
);

afterEach(async () => {
  for (const dir of tempDirs.splice(0)) {
    await rm(dir, { recursive: true, force: true, maxRetries: 5, retryDelay: 20 });
  }
});

async function makeTempDir(): Promise<string> {
  const dir = await mkdtemp(join(tmpdir(), 'kimi-sdk-create-'));
  tempDirs.push(dir);
  return dir;
}

async function writeTestModelConfig(homeDir: string): Promise<void> {
  await writeFile(join(homeDir, 'config.toml'), `
default_model = "alias-model"

[providers.local]
type = "openai"
base_url = "https://example.test/v1"
api_key = "sk-test"

[models.alias-model]
provider = "local"
model = "real-model"
max_context_size = 1000
`, 'utf-8');
}

class StubRpc extends SDKRpcClientBase {
  readonly resumeCalls: Array<{ input: ResumeSessionInput; kaos: Kaos; persistenceKaos?: Kaos }> = [];

  override async createSession(input: { id?: string; workDir: string }) {
    return {
      id: input.id ?? 'ses_stub',
      workDir: input.workDir,
      sessionDir: '/tmp/session',
      createdAt: 1,
      updatedAt: 1,
    };
  }

  override async resumeSessionWithKaos(input: ResumeSessionInput, kaos: Kaos, persistenceKaos?: Kaos): Promise<ResumedSessionSummary> {
    this.resumeCalls.push({ input, kaos, persistenceKaos });
    return {
      id: input.id,
      workDir: '/tmp/work',
      sessionDir: '/tmp/session',
      createdAt: 1,
      updatedAt: 1,
      sessionMetadata: {
        id: input.id,
        version: 2,
        cwd: '/tmp/work',
        createdAt: 0,
        updatedAt: 0,
        title: '',
        isCustomTitle: false,
        archived: false,
        agents: {},
        custom: {},
      },
      agents: {},
    };
  }
}

describe('KimiHarness session lifecycle', () => {
  it('attributes created and resumed sessions to the host', async () => {
    const records: TelemetryRecord[] = [];
    const harness = createKimiHarness({
      identity: TEST_IDENTITY,
      homeDir: await makeTempDir(),
      uiMode: 'print',
      telemetry: recordingTelemetry(records),
      sessionStartedProperties: { source: 'process' },
    });
    const workDir = await makeTempDir();
    try {
      const session = await harness.createSession({
        id: 'ses_session_started',
        workDir,
        sessionStartedProperties: { source: 'session' },
      });
      expect(records).toContainEqual({
        event: 'session_started',
        sessionId: session.id,
        properties: expect.objectContaining({
          client_name: 'kimi-code-cli',
          client_version: '0.0.0-test',
          ui_mode: 'print',
          resumed: false,
          source: 'session',
        }),
      });

      await session.close();
      await harness.resumeSession({ id: session.id });
      expect(records).toContainEqual({
        event: 'session_started',
        sessionId: session.id,
        properties: expect.objectContaining({ resumed: true }),
      });
    } finally {
      await harness.close();
    }
  });

  it('creates, lists, closes, and resumes one stable session identity', async () => {
    const harness = createKimiHarness({ identity: TEST_IDENTITY, homeDir: await makeTempDir() });
    const workDir = await makeTempDir();
    try {
      const created = await harness.createSession({ id: 'ses_lifecycle', workDir });
      expect(harness.getSession(created.id)).toBe(created);
      expect((await harness.listSessions({ workDir })).map((item) => item.id)).toEqual([created.id]);

      await created.close();
      expect(harness.getSession(created.id)).toBeUndefined();
      const resumed = await harness.resumeSession({ id: created.id });
      expect(resumed.id).toBe(created.id);
      expect(harness.getSession(created.id)).toBe(resumed);
    } finally {
      await harness.close();
    }
  });

  it('applies caller MCP servers when creating and resuming sessions', async () => {
    const harness = createKimiHarness({ identity: TEST_IDENTITY, homeDir: await makeTempDir() });
    const workDir = await makeTempDir();
    try {
      const created = await harness.createSession({
        id: 'ses_caller_mcp',
        workDir,
        mcpServers: {
          created: { transport: 'stdio', command: process.execPath, args: [stdioFixture] },
        },
      });
      await expect(created.listMcpServers()).resolves.toEqual([
        { name: 'created', transport: 'stdio', status: 'connected', toolCount: 3 },
      ]);

      await created.close();
      const resumed = await harness.resumeSession({
        id: created.id,
        mcpServers: {
          resumed: { transport: 'stdio', command: process.execPath, args: [stdioFixture] },
        },
      });
      await expect(resumed.listMcpServers()).resolves.toEqual([
        { name: 'resumed', transport: 'stdio', status: 'connected', toolCount: 3 },
      ]);
    } finally {
      await harness.close();
    }
  }, 15_000);

  it('uses configured model aliases and explicit runtime options', async () => {
    const homeDir = await makeTempDir();
    await writeTestModelConfig(homeDir);
    const harness = createKimiHarness({ identity: TEST_IDENTITY, homeDir });
    try {
      const session = await harness.createSession({
        id: 'ses_runtime_options',
        workDir: await makeTempDir(),
        thinking: 'low',
        permission: 'auto',
      });

      await expect(session.getStatus()).resolves.toMatchObject({
        model: 'alias-model',
        thinkingEffort: 'low',
        permission: 'auto',
      });
    } finally {
      await harness.close();
    }
  });

  it('does not require provider credentials until a model request is made', async () => {
    const harness = createKimiHarness({ identity: TEST_IDENTITY, homeDir: await makeTempDir() });
    try {
      const session = await harness.createSession({ id: 'ses_empty_config', workDir: await makeTempDir() });
      expect((await session.getStatus()).model).toBeUndefined();
    } finally {
      await harness.close();
    }
  });

  it('requires a non-empty workspace path', async () => {
    const harness = createKimiHarness({ identity: TEST_IDENTITY, homeDir: await makeTempDir() });
    try {
      await expect(harness.createSession({ id: 'ses_missing_workdir' } as never)).rejects.toMatchObject({
        code: 'request.work_dir_required',
      } satisfies Partial<KimiError>);
      await expect(harness.createSession({ id: 'ses_blank_workdir', workDir: '   ' })).rejects.toMatchObject({
        code: 'request.work_dir_required',
      } satisfies Partial<KimiError>);
    } finally {
      await harness.close();
    }
  });

  it('rebinds an active session when resume receives a new Kaos', async () => {
    const rpc = new StubRpc();
    const harness = new KimiHarness(rpc, {
      homeDir: '/tmp/home',
      configPath: '/tmp/config.toml',
      auth: { status: async () => ({ providers: [] }) } as never,
      telemetry: recordingTelemetry([]),
      ensureConfigFile: async () => undefined,
      onClose: () => undefined,
    });
    const session = await harness.createSession({ id: 'ses_active', workDir: '/tmp/work' });
    const kaos = {} as Kaos;

    await expect(harness.resumeSession({ id: session.id, kaos })).resolves.toBe(session);
    expect(rpc.resumeCalls).toEqual([{ input: { id: session.id }, kaos, persistenceKaos: undefined }]);
  });
});
