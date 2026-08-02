import { defineConfig } from 'vitest/config';

export default defineConfig({
  test: {
    name: 'dimi-native',
    include: ['test/**/*.test.ts'],
    setupFiles: ['./vitest.setup.ts'],
  },
});
