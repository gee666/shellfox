import assert from 'node:assert/strict';
import { test } from 'node:test';
import { inspectUnitSource } from './check-unit-sources.mjs';

const file = 'src/sample.test.ts';
test('rejects live application launch routes and partial process mocks at parse time', () => {
  for (const snippet of [
    `import { spawn } from 'node:child_process';`,
    `await import('child_process');`,
    `vi.mock('node:child_process', async actual => ({ ...await actual(), spawn: vi.fn() }));`,
    `vi.mock('child_process', () => vi.importActual('child_process'));`,
    `vi.unmock('node:child_process');`,
    `const load = require('node:child_process');`,
    `import { createRequire as load } from 'module';`,
    `process['kill'](42, 0);`,
    `globalThis.process.getBuiltinModule('child_process');`,
    `new PtyBackend().launch(input);`,
    `import { chromium } from '@playwright/test';`,
    `await startHandoff(process.execPath, ['helper.cjs'], root);`,
    `import './sample.os.test';`,
    `import '../tests/fixtures/ssh-e2e-driver';`,
    `import './old-integration.test';`,
  ]) assert.ok(inspectUnitSource(file, snippet, ['src/old-integration.test.ts']).problems.length, snippet);
});

test('admits a local process double, type-only bindings and in-process fixture children', () => {
  const result = inspectUnitSource(file, `
    import type { ChildProcess } from 'child_process';
    import { spawn } from 'node:child_process';
    vi.mock('node:child_process', () => ({ spawn: vi.fn() }));
    for (const process of fixtures) expect(process.kill).not.toHaveBeenCalled();
  `);
  assert.deepEqual(result.problems, []);
});
