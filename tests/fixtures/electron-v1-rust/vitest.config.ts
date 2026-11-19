import { defineConfig } from 'vitest/config';
import { textAssets } from '../../config/text-assets';

// From the repo root, set SHELLFOX_TEST_RUST_HOST_CLIENT to the executable from
// `cargo test -p shellfox-desktop --test host-client --no-run`, then run:
// pnpm exec vitest run --config tests/fixtures/electron-v1-rust/vitest.config.ts
export default defineConfig({
  plugins: [textAssets],
  cacheDir: 'tmp/build-cache/vitest',
  test: { include: ['tests/fixtures/electron-v1-rust/interop.test.ts'], maxWorkers: 1 },
});
