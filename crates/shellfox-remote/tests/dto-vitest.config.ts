import { defineConfig } from 'vitest/config';

export default defineConfig({
  cacheDir: 'tmp/build-cache/vitest',
  test: {
    include: ['crates/shellfox-remote/tests/dto-fixtures.test.ts'],
    environment: 'node',
    maxWorkers: 1,
  },
});
