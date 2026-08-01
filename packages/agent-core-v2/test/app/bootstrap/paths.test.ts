import { existsSync, mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

import { afterEach, describe, expect, it } from 'vitest';

import { ensureDimiHome, resolveConfigPath, resolveDimiHome } from '#/app/bootstrap/bootstrap';

describe('bootstrap path helpers', () => {
  describe('resolveDimiHome', () => {
    it('uses explicit homeDir when provided', () => {
      expect(resolveDimiHome('/tmp/dimi')).toBe('/tmp/dimi');
    });

    it('falls back to DIMI_CODE_HOME env', () => {
      const prev = process.env['DIMI_CODE_HOME'];
      process.env['DIMI_CODE_HOME'] = '/env/dimi';
      try {
        expect(resolveDimiHome()).toBe('/env/dimi');
      } finally {
        if (prev === undefined) delete process.env['DIMI_CODE_HOME'];
        else process.env['DIMI_CODE_HOME'] = prev;
      }
    });
  });

  describe('resolveConfigPath', () => {
    it('uses explicit configPath when provided', () => {
      expect(resolveConfigPath({ configPath: '/x/config.toml' })).toBe('/x/config.toml');
    });

    it('joins homeDir with config.toml', () => {
      expect(resolveConfigPath({ homeDir: '/tmp/dimi' })).toBe('/tmp/dimi/config.toml');
    });
  });

  describe('ensureDimiHome', () => {
    let dir: string | undefined;
    afterEach(() => {
      if (dir) rmSync(dir, { recursive: true, force: true });
    });

    it('creates the directory with 0700 permissions', () => {
      dir = join(mkdtempSync(join(tmpdir(), 'dimi-home-')), 'nested');
      ensureDimiHome(dir);
      expect(existsSync(dir)).toBe(true);
    });
  });
});
