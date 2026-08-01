import { describe, expect, it } from 'vitest';

import { SRC_ROOT, checkSource } from '../../scripts/check-domain-layers.mjs';

const at = (domain: string, file: string): string => `${SRC_ROOT}/${domain}/${file}`;

const REMOVED_RUNTIME = ['@dimi-agent', 'agent-core'].join('/');

describe('check-domain-layers', () => {
  it('flags a direct import of the removed runtime package', () => {
    const violations = checkSource(
      `import { DimiCore } from '${REMOVED_RUNTIME}';`,
      at('loop', 'loop.ts'),
    );
    expect(violations).toHaveLength(1);
    expect(violations[0]?.message).toMatch(/current runtime must not import removed runtime/);
  });

  it('flags a subpath import of the removed runtime package', () => {
    const violations = checkSource(
      `import { Session } from '${REMOVED_RUNTIME}/session';`,
      at('loop', 'loop.ts'),
    );
    expect(violations).toHaveLength(1);
    expect(violations[0]?.message).toMatch(/current runtime must not import removed runtime/);
  });

  it('allows a domain to import a lower layer', () => {
    const violations = checkSource(
      `import { createDecorator } from '#/_base/di/instantiation';`,
      at('loop', 'loop.ts'),
    );
    expect(violations).toHaveLength(0);
  });

  it('flags a lower layer importing a higher layer', () => {
    const violations = checkSource(
      `import { IAgentLoopService } from '#/agent/loop/loop';`,
      at('log', 'log.ts'),
    );
    expect(violations).toHaveLength(1);
    expect(violations[0]?.message).toMatch(/layer violation/);
    expect(violations[0]?.message).toMatch(/log.*L1.*loop.*L4/s);
  });

  it('allows same-domain relative imports', () => {
    const violations = checkSource(
      `import { helper } from './helper';`,
      at('loop', 'loop.ts'),
    );
    expect(violations).toHaveLength(0);
  });

  it('allows sibling-package imports (out of scope for layering)', () => {
    const violations = checkSource(
      `import { something } from '@dimi-agent/kaos';`,
      at('log', 'log.ts'),
    );
    expect(violations).toHaveLength(0);
  });

  it('exempts the top-level package barrel from layering', () => {
    const violations = checkSource(
      `export * from './_base/di/index';`,
      `${SRC_ROOT}/index.ts`,
    );
    expect(violations).toHaveLength(0);
  });
});
