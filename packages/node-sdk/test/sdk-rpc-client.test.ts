/**
 * Scenario: the public SDK talks to the in-process runtime over klient memory
 * transport. No provider calls are made.
 */
import { mkdir, mkdtemp, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

import { afterEach, describe, expect, it } from 'vitest';

import { createDimiHarness, ErrorCodes, DimiError, DimiHarness, SDKRpcClient } from '#/index';

import { TEST_IDENTITY } from './test-identity';
import { recordingTelemetry, type TelemetryRecord } from './telemetry';

const tempDirs: string[] = [];

afterEach(async () => {
  for (const dir of tempDirs.splice(0)) {
    await rm(dir, { recursive: true, force: true });
  }
});

async function makeHarness(): Promise<{ harness: DimiHarness; homeDir: string }> {
  const homeDir = await mkdtemp(join(tmpdir(), 'dimi-sdk-'));
  tempDirs.push(homeDir);
  return { harness: createDimiHarness({ homeDir, identity: TEST_IDENTITY }), homeDir };
}

describe('SDKRpcClient', () => {
  it('serves getExperimentalFeatures from the runtime', async () => {
    const { harness } = await makeHarness();
    try {
      const features = await harness.getExperimentalFeatures();
      expect(Array.isArray(features)).toBe(true);
      expect(features.length).toBeGreaterThan(0);
      for (const feature of features) {
        expect(typeof feature.id).toBe('string');
        expect(typeof feature.title).toBe('string');
        expect(typeof feature.env).toBe('string');
        expect(typeof feature.enabled).toBe('boolean');
        expect(typeof feature.defaultEnabled).toBe('boolean');
      }
    } finally {
      await harness.close();
    }
  });

  it('serves listWorkspaceSkills', async () => {
    const { harness, homeDir } = await makeHarness();
    const workDir = await mkdtemp(join(tmpdir(), 'dimi-sdk-work-'));
    tempDirs.push(workDir);
    await writeSkill(join(homeDir, 'skills', 'demo-user-skill'), 'demo-user-skill');
    await writeSkill(join(workDir, '.dimi', 'skills', 'demo-project-skill'), 'demo-project-skill');
    try {
      const skills = await harness.listWorkspaceSkills(workDir);
      const byName = new Map(skills.map((skill) => [skill.name, skill]));
      expect(byName.get('demo-user-skill')).toMatchObject({
        description: 'Skill demo-user-skill for the escape-hatch test',
        source: 'user',
      });
      expect(byName.get('demo-project-skill')).toMatchObject({
        description: 'Skill demo-project-skill for the escape-hatch test',
        source: 'project',
      });
    } finally {
      await harness.close();
    }
  });

  it('honors skillDirs (explicit dirs) over default user / project discovery', async () => {
    const homeDir = await mkdtemp(join(tmpdir(), 'dimi-sdk-'));
    tempDirs.push(homeDir);
    const workDir = await mkdtemp(join(tmpdir(), 'dimi-sdk-work-'));
    tempDirs.push(workDir);
    const explicitBase = await mkdtemp(join(tmpdir(), 'dimi-sdk-explicit-'));
    tempDirs.push(explicitBase);
    const explicitDir = join(explicitBase, 'skills');
    await writeSkill(join(homeDir, 'skills', 'demo-user-skill'), 'demo-user-skill');
    await writeSkill(join(workDir, '.dimi', 'skills', 'demo-project-skill'), 'demo-project-skill');
    await writeSkill(join(explicitDir, 'demo-explicit-skill'), 'demo-explicit-skill');
    const harness = createDimiHarness({
      homeDir,
      identity: TEST_IDENTITY,
      skillDirs: [explicitDir],
    });
    try {
      const skills = await harness.listWorkspaceSkills(workDir);
      const byName = new Map(skills.map((skill) => [skill.name, skill]));
      expect(byName.get('demo-explicit-skill')).toMatchObject({
        description: 'Skill demo-explicit-skill for the escape-hatch test',
        source: 'user',
      });
      expect(byName.has('demo-user-skill')).toBe(false);
      expect(byName.has('demo-project-skill')).toBe(false);

      // The session skill catalog (the Skill tool's listing) goes through the
      // seeded engine runtime options, so it sees the same explicit source.
      const session = await harness.createSession({ workDir });
      const sessionNames = new Set((await session.listSkills()).map((skill) => skill.name));
      expect(sessionNames.has('demo-explicit-skill')).toBe(true);
      expect(sessionNames.has('demo-user-skill')).toBe(false);
      expect(sessionNames.has('demo-project-skill')).toBe(false);
      await session.close();
    } finally {
      await harness.close();
    }
  });

  it('serves the plugin catalog on an empty home', async () => {
    const homeDir = await mkdtemp(join(tmpdir(), 'dimi-sdk-'));
    tempDirs.push(homeDir);
    const rpc = new SDKRpcClient({ homeDir, identity: TEST_IDENTITY });
    try {
      expect(await rpc.listPlugins()).toEqual([]);
      expect(await rpc.reloadPlugins()).toEqual({ added: [], removed: [], errors: [] });
      await expect(rpc.getPluginInfo('missing-plugin')).rejects.toThrow();
    } finally {
      await rpc.close();
    }
  });

  it('fails loudly for unsupported session deletion', async () => {
    const { harness } = await makeHarness();
    try {
      await expect(harness.deleteSession('session_missing')).rejects.toThrowError(DimiError);
      await expect(harness.deleteSession('session_missing')).rejects.toMatchObject({
        code: ErrorCodes.NOT_IMPLEMENTED,
      });
    } finally {
      await harness.close();
    }
  });

  it('composes persisted models.json layers over SDK providers', async () => {
    const homeDir = await mkdtemp(join(tmpdir(), 'dimi-sdk-provider-'));
    tempDirs.push(homeDir);
    await writeFile(
      join(homeDir, 'models.json'),
      JSON.stringify({
        providers: {
          extension: {
            name: 'Persisted definition',
            api: 'openai-completions',
            baseUrl: 'https://persisted.example.test/v1',
            models: [{ id: 'persisted', contextWindow: 8_192, maxTokens: 1_024 }],
          },
        },
      }),
      'utf8',
    );
    const harness = createDimiHarness({
      homeDir,
      identity: TEST_IDENTITY,
      providers: [{
        id: 'extension',
        name: 'SDK extension',
        baseUrl: 'https://extension.example.test/v1',
        auth: {
          apiKey: {
            name: 'Extension key',
            resolve: async () => ({ auth: { apiKey: 'test-key' }, source: 'test' }),
          },
        },
        getModels: () => [{
          id: 'extension-chat',
          name: 'Extension chat',
          api: 'openai-completions',
          provider: 'extension',
          baseUrl: 'https://extension.example.test/v1',
          reasoning: true,
          input: ['text'],
          cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 },
          contextWindow: 16_384,
          maxTokens: 2_048,
        }],
        stream: async function* () {},
      }],
    });
    try {
      await expect(harness.auth.providers()).resolves.toContainEqual(
        expect.objectContaining({ id: 'extension', name: 'Persisted definition' }),
      );
      await expect(harness.auth.models('extension')).resolves.toEqual(expect.arrayContaining([
        expect.objectContaining({ id: 'extension-chat', baseUrl: 'https://persisted.example.test/v1' }),
        expect.objectContaining({ id: 'persisted', baseUrl: 'https://persisted.example.test/v1' }),
      ]));
    } finally {
      await harness.close();
    }
  });
});

describe('SDKRpcClient engine telemetry', () => {
  it('forwards engine-side events to the host-supplied telemetry client', async () => {
    const homeDir = await mkdtemp(join(tmpdir(), 'dimi-sdk-tel-'));
    tempDirs.push(homeDir);
    const workDir = await mkdtemp(join(tmpdir(), 'dimi-sdk-tel-work-'));
    tempDirs.push(workDir);
    const records: TelemetryRecord[] = [];
    const harness = createDimiHarness({
      homeDir,
      identity: TEST_IDENTITY,
      telemetry: recordingTelemetry(records),
    });
    try {
      const session = await harness.createSession({ workDir });
      await session.setPermission('yolo');
      expect(records.some((record) => record.event === 'yolo_toggle')).toBe(true);
      await session.close();
    } finally {
      await harness.close();
    }
  });

  it('honors telemetry = false for engine-side events', async () => {
    const homeDir = await mkdtemp(join(tmpdir(), 'dimi-sdk-tel-off-'));
    tempDirs.push(homeDir);
    const workDir = await mkdtemp(join(tmpdir(), 'dimi-sdk-tel-off-work-'));
    tempDirs.push(workDir);
    await writeFile(join(homeDir, 'config.toml'), 'telemetry = false\n', 'utf-8');
    const records: TelemetryRecord[] = [];
    const harness = createDimiHarness({
      homeDir,
      identity: TEST_IDENTITY,
      telemetry: recordingTelemetry(records),
    });
    try {
      const session = await harness.createSession({ workDir });
      await session.setPermission('yolo');
      expect(records.some((record) => record.event === 'yolo_toggle')).toBe(false);
      await session.close();
    } finally {
      await harness.close();
    }
  });
});

async function writeSkill(dir: string, name: string): Promise<void> {
  await mkdir(dir, { recursive: true });
  await writeFile(
    join(dir, 'SKILL.md'),
    `---\nname: ${name}\ndescription: Skill ${name} for the escape-hatch test\n---\n\nBody of ${name}.\n`,
    'utf-8',
  );
}
