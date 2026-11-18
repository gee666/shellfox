import { configDefaults, defineConfig } from 'vitest/config';
import { textAssets } from './tests/config/text-assets';
import { unitExcludes } from './tests/config/integration-registry';
// In-process app contracts. Fixture file/loopback I/O is not native ACL acceptance.
export default defineConfig({ plugins: [textAssets], cacheDir: 'tmp/build-cache/vitest', test: {
  include: ['src/**/*.test.{ts,tsx}', 'tests/unit/**/*.test.ts', 'mobile/src/**/*.test.{ts,tsx}'],
  exclude: [...configDefaults.exclude, ...unitExcludes], setupFiles: ['tests/unit/setup-app-boundary.ts'],
  environment: 'node', maxWorkers: 2, server: { deps: { external: ['react-native'] } },
} });
