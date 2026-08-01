/**
 * Dimi version helpers.
 *
 * `getVersion` reads the host CLI's `package.json#version`.
 */

import { existsSync, readFileSync } from 'node:fs';
import { dirname, resolve } from 'node:path';

import { createDimiDefaultHeaders, createDimiUserAgent, type DimiHostIdentity } from '@dimi-agent/dimi-oauth';

import { CLI_USER_AGENT_PRODUCT } from '#/constant/app';

import { getDataDir } from '../utils/paths';
import { DIMI_BUILD_INFO } from './build-info';

const MODULE_DIR = import.meta.dirname;

export function getHostPackageJsonPath(): string {
  // Walk upwards from this file's directory until a `package.json` shows up,
  // so both dev (`tsx src/main.ts` — this file in `src/cli/`, pkg 2 levels
  // up) and prod (`node dist/main.mjs` — this code bundled into `dist/`,
  // pkg 1 level up) resolve correctly.
  let dir = MODULE_DIR;
  for (let i = 0; i < 6; i++) {
    const candidate = resolve(dir, 'package.json');
    if (existsSync(candidate)) {
      return candidate;
    }
    const parent = dirname(dir);
    if (parent === dir) break;
    dir = parent;
  }
  throw new Error(`Could not locate package.json near ${MODULE_DIR}`);
}

export function getHostPackageRoot(): string {
  return dirname(getHostPackageJsonPath());
}

export function getVersion(): string {
  if (DIMI_BUILD_INFO.version !== undefined) {
    return DIMI_BUILD_INFO.version;
  }
  const pkg = JSON.parse(readFileSync(getHostPackageJsonPath(), 'utf-8')) as {
    version: string;
  };
  return pkg.version;
}

export function createDimiCodeHostIdentity(version = getVersion()): DimiHostIdentity {
  return {
    userAgentProduct: CLI_USER_AGENT_PRODUCT,
    version,
  };
}

/**
 * Product User-Agent (`dimi-cli/<version>`) for ad-hoc outbound fetches
 * that don't go through the provider pipeline (registry / catalog imports).
 */
export function createDimiCodeUserAgent(version = getVersion()): string {
  return createDimiUserAgent(createDimiCodeHostIdentity(version));
}

export function buildDimiDefaultHeaders(version: string): Record<string, string> {
  return createDimiDefaultHeaders({
    homeDir: getDataDir(),
    ...createDimiCodeHostIdentity(version),
  });
}
