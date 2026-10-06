import { readFileSync } from 'node:fs';
import { defineConfig } from 'vitest/config';
// Update scripts are imported as text (esbuild uses the same 'text' loader).
const textAssets = { name: 'text-assets', enforce: 'pre' as const, transform: (_code: string, id: string) => /\.(sh|ps1)(\?.*)?$/.test(id) ? { code: 'export default ' + JSON.stringify(readFileSync(id.replace(/\?.*$/, ''), 'utf8')) + ';', map: null } : null };
export default defineConfig({ plugins: [textAssets], cacheDir: 'tmp/build-cache/vitest', test: { include: ['src/**/*.test.{ts,tsx}', 'tests/unit/**/*.test.ts'], environment: 'node', maxWorkers: 2 } });
