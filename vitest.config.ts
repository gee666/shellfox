import { defineConfig } from 'vitest/config';
export default defineConfig({ cacheDir: 'tmp/build-cache/vitest', test: { include: ['src/**/*.test.{ts,tsx}', 'tests/unit/**/*.test.ts'], environment: 'node', maxWorkers: 2 } });
