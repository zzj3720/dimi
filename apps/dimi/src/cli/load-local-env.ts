/**
 * Local `.env` loading for the Dimi CLI.
 *
 * Dimi reads `OPENCODE_API_KEY` and other provider credentials from the
 * environment. For local development we also load a `.env` file so secrets can
 * live in the repo root (gitignored) instead of being exported by hand.
 *
 * Lookup order — first file found wins:
 *   1. `<cwd>/.env` then walk up parent directories (repo root discovery)
 *   2. `<dimiHome>/.env` (e.g. `~/.dimi/.env`)
 *
 * Walking up matters because package managers run with the package directory
 * as cwd (`pnpm --filter … exec`), while the checked-in `.env` lives at the
 * repository root.
 *
 * Only variables that are NOT already set in the environment are applied, so
 * a shell export always overrides the file. Missing files and unreadable files
 * are silently ignored.
 */
import { existsSync, readFileSync } from 'node:fs';
import { join, parse } from 'node:path';

import { resolveDimiHome } from '@dimi-agent/dimi-sdk';

export function loadLocalEnv(env: NodeJS.ProcessEnv = process.env): void {
  for (const path of envCandidates()) {
    if (!existsSync(path)) continue;
    try {
      applyEnvFile(path, env);
      return;
    } catch {
      // A malformed .env must never block CLI startup; fall through silently.
    }
  }
}

function envCandidates(): readonly string[] {
  const cwd = process.cwd();
  const candidates: string[] = [];
  let dir = cwd;
  for (let depth = 0; depth < 8 && dir.length > 0; depth += 1) {
    candidates.push(join(dir, '.env'));
    const parent = parse(dir).dir;
    if (parent === dir) break;
    dir = parent;
  }
  candidates.push(join(resolveDimiHome(), '.env'));
  return candidates;
}

function applyEnvFile(path: string, env: NodeJS.ProcessEnv): void {
  const text = readFileSync(path, 'utf8');
  for (const rawLine of text.split('\n')) {
    const line = rawLine.trim();
    if (line.length === 0 || line.startsWith('#')) continue;
    const eq = line.indexOf('=');
    if (eq <= 0) continue;
    const key = line.slice(0, eq).trim();
    let value = line.slice(eq + 1).trim();
    if (value.length >= 2 && value.startsWith('"') && value.endsWith('"')) {
      value = value.slice(1, -1);
    }
    if (key.length === 0 || env[key] !== undefined) continue;
    env[key] = value;
  }
}
