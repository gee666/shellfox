import { defineConfig } from '@playwright/test';
import path from 'node:path';
export default defineConfig({
  testDir: './routing-tests', outputDir: path.resolve('tmp/routing-playwright/artifacts'),
  timeout: 120000, workers: 1, retries: 0, fullyParallel: false,
  reporter: [['list'], ['json', { outputFile: path.resolve('tmp/routing-playwright/report.json') }]],
  projects: [{ name: 'windows' }],
});
