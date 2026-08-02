/**
 * `--legacy` switch tests — the "use the TypeScript backend" flag.
 *
 * The main command parses `--legacy` and arms the process-wide
 * `DIMI_LEGACY_STORE` env switch (any server started in this process honors
 * it), and the web server CLI options carry `legacyStore` through
 * `parseServerOptions` into `startServer`.
 */
import { afterEach, describe, expect, test } from 'vitest';

import { createProgram } from '#/cli/commands';
import { parseServerOptions } from '#/cli/sub/web/shared';

describe('--legacy flag', () => {
  afterEach(() => {
    delete process.env['DIMI_LEGACY_STORE'];
  });

  test('main command parses --legacy and sets the env switch', () => {
    let seenLegacy: boolean | undefined;
    const program = createProgram(
      '0.0.0-test',
      (opts) => {
        seenLegacy = opts.legacy;
      },
      () => {},
      () => {},
    );
    program.exitOverride();
    program.parse(['--legacy'], { from: 'user' });
    expect(seenLegacy).toBe(true);
    expect(process.env['DIMI_LEGACY_STORE']).toBe('1');
  });

  test('main command without --legacy leaves the env switch unset', () => {
    let seenLegacy: boolean | undefined;
    const program = createProgram(
      '0.0.0-test',
      (opts) => {
        seenLegacy = opts.legacy;
      },
      () => {},
      () => {},
    );
    program.exitOverride();
    program.parse([], { from: 'user' });
    expect(seenLegacy).toBe(false);
    expect(process.env['DIMI_LEGACY_STORE']).toBeUndefined();
  });

  test('web server options carry legacyStore', () => {
    expect(parseServerOptions({ legacyStore: true }).legacyStore).toBe(true);
    expect(parseServerOptions({}).legacyStore).toBe(false);
  });
});
