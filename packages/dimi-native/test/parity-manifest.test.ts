/**
 * Parity manifest gate (A5 architecture review).
 *
 * Refuses unclaimed ledger entries: a migrated domain must name its coverage
 * test, an unmigrated domain must carry an acknowledged gap description. The
 * ledger itself lives in `parity-manifest.ts`; fixing a gap = flipping the
 * entry + landing the coverage test.
 */
import { describe, expect, test } from 'vitest';

import { PARITY_MANIFEST } from './parity-manifest';

describe('parity manifest', () => {
  test('every migrated domain names its coverage test', () => {
    for (const entry of PARITY_MANIFEST) {
      if (!entry.migrated) continue;
      expect(entry.coveredBy, `${entry.domain} must name its coverage`).toBeTruthy();
    }
  });

  test('every unmigrated domain carries an acknowledged gap', () => {
    for (const entry of PARITY_MANIFEST) {
      if (entry.migrated) continue;
      expect(entry.gap, `${entry.domain} must carry an acknowledged gap`).toBeTruthy();
    }
  });

  test('entries are unique by domain', () => {
    const names = PARITY_MANIFEST.map((entry) => entry.domain);
    expect(new Set(names).size).toBe(names.length);
  });
});
