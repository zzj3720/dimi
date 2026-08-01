import { mkdir, mkdtemp, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { loadLocalEnv } from '#/cli/load-local-env';

// loadLocalEnv resolves the Dimi home via resolveDimiHome; isolate it.
vi.mock('@dimi-agent/dimi-sdk', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@dimi-agent/dimi-sdk')>();
  let homeDir = '/nonexistent/dimi-home';
  return {
    ...actual,
    resolveDimiHome: () => homeDir,
    __setHome: (value: string) => {
      homeDir = value;
    },
  };
});

import { resolveDimiHome } from '@dimi-agent/dimi-sdk';
import type { Mock } from 'vitest';

describe('loadLocalEnv', () => {
  let cwdSpy: Mock;
  let dir: string;
  let homeDir: string;
  let setHome: (value: string) => void;

  beforeEach(async () => {
    dir = await mkdtemp(join(tmpdir(), 'dimi-env-test-'));
    homeDir = join(dir, 'home');
    cwdSpy = vi.spyOn(process, 'cwd').mockReturnValue(dir);
    const sdk = await import('@dimi-agent/dimi-sdk');
    setHome = (sdk as unknown as { __setHome: (v: string) => void }).__setHome;
    setHome(homeDir);
  });

  afterEach(async () => {
    cwdSpy.mockRestore();
    vi.restoreAllMocks();
    await rm(dir, { recursive: true, force: true });
  });

  it('loads OPENCODE_API_KEY from the cwd .env', async () => {
    await writeFile(join(dir, '.env'), 'OPENCODE_API_KEY=sk-test-123\n');
    const env: NodeJS.ProcessEnv = {};
    loadLocalEnv(env);
    expect(env['OPENCODE_API_KEY']).toBe('sk-test-123');
  });

  it('walks up parent directories to find a repo-root .env', async () => {
    // Simulate `pnpm --filter … exec`: cwd is the package dir, .env is at root.
    const pkgDir = join(dir, 'apps', 'dimi');
    await mkdir(pkgDir, { recursive: true });
    await writeFile(join(dir, '.env'), 'OPENCODE_API_KEY=sk-root-123\n');
    cwdSpy.mockReturnValue(pkgDir);
    const env: NodeJS.ProcessEnv = {};
    loadLocalEnv(env);
    expect(env['OPENCODE_API_KEY']).toBe('sk-root-123');
  });

  it('falls back to the dimi home .env when cwd has none', async () => {
    await mkdir(homeDir);
    await writeFile(join(homeDir, '.env'), 'OPENCODE_API_KEY=sk-home-456\n');
    const env: NodeJS.ProcessEnv = {};
    loadLocalEnv(env);
    expect(env['OPENCODE_API_KEY']).toBe('sk-home-456');
  });

  it('does not override an already-set environment variable', async () => {
    await writeFile(join(dir, '.env'), 'OPENCODE_API_KEY=sk-file\n');
    const env: NodeJS.ProcessEnv = { OPENCODE_API_KEY: 'sk-shell' };
    loadLocalEnv(env);
    expect(env['OPENCODE_API_KEY']).toBe('sk-shell');
  });

  it('strips quotes and skips comments and blank lines', async () => {
    await writeFile(
      join(dir, '.env'),
      ['# a comment', '', '  ', 'OPENCODE_API_KEY="sk-quoted-789"', 'EMPTY_VAR='].join('\n'),
    );
    const env: NodeJS.ProcessEnv = {};
    loadLocalEnv(env);
    expect(env['OPENCODE_API_KEY']).toBe('sk-quoted-789');
    expect(env['EMPTY_VAR']).toBe('');
  });

  it('is a no-op when no .env exists', async () => {
    const env: NodeJS.ProcessEnv = {};
    expect(() => loadLocalEnv(env)).not.toThrow();
    expect(Object.keys(env)).toHaveLength(0);
  });
});
