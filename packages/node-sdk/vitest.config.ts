import { fileURLToPath } from 'node:url';

import { defineConfig } from 'vitest/config';

export default defineConfig({
  resolve: {
    alias: {
      '@dimi-agent/dimi-oauth': fileURLToPath(
        new URL('../oauth/src/index.ts', import.meta.url),
      ),
    },
  },
  test: {
    name: 'dimi-sdk',
    env: {
      DIMI_LOG_LEVEL: 'off',
    },
    include: ['test/**/*.test.ts'],
  },
});
