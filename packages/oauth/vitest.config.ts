import { defineConfig } from 'vitest/config';

export default defineConfig({
  test: {
    name: 'dimi-oauth',
    include: ['test/**/*.test.ts'],
  },
});
