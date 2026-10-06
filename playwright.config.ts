import { defineConfig } from '@playwright/test';
const requested = process.env.SHELLFOX_VERIFY_PROJECT;
const suite = requested && ['e2e', 'windows', 'packaged'].includes(requested) ? requested : 'all';
export default defineConfig({
  testDir: './tests', outputDir: `./tmp/reports/playwright/${suite}/artifacts`,
  reporter: [['list'], ['json', { outputFile: `tmp/reports/playwright/${suite}/results.json` }]],
  workers: 1, fullyParallel: false, retries: 0, timeout: 60000,
  expect: { timeout: 10000 },
  projects: [
    { name: 'e2e', testMatch: 'e2e/**/*.spec.ts' },
    { name: 'windows', testMatch: 'windows/**/*.spec.ts', timeout: 300000 },
    { name: 'packaged', testMatch: 'packaged/**/*.spec.ts' },
  ],
});
