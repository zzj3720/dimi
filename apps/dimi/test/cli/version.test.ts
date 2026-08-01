import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';

import { describe, expect, it } from 'vitest';

import {
  buildDimiDefaultHeaders,
  createDimiCodeUserAgent,
  getHostPackageJsonPath,
  getHostPackageRoot,
  getVersion,
} from '#/cli/version';

describe('cli version helpers', () => {
  it('resolves the host package manifest near apps/dimi and reads its version', () => {
    const pkgPath = getHostPackageJsonPath();
    const pkg = JSON.parse(readFileSync(pkgPath, 'utf8')) as { version: string };

    expect(pkgPath.endsWith(join('apps', 'dimi', 'package.json'))).toBe(true);
    expect(getHostPackageRoot()).toBe(dirname(pkgPath));
    expect(getVersion()).toBe(pkg.version);
  });

  it('builds default headers with the dimi-cli user-agent', () => {
    const headers = buildDimiDefaultHeaders('1.2.3');

    expect(headers['User-Agent']).toBe('dimi-cli/1.2.3');
  });

  it('builds the product user-agent for ad-hoc fetches', () => {
    expect(createDimiCodeUserAgent('1.2.3')).toBe('dimi-cli/1.2.3');
  });
});
