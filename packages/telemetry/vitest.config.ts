import { defineConfig } from 'vitest/config';

export default defineConfig({
  test: {
    name: 'dimi-telemetry',
    include: ['test/**/*.test.ts'],
  },
});
