import { defineConfig } from 'vitest/config';

export default defineConfig({
  test: {
    name: 'mobile',
    include: ['test/**/*.test.ts'],
  },
});
