// Production startup/preload smoke only. Creates no shells and uses isolated data.
import assert from 'node:assert/strict';
import path from 'node:path';
import { mkdirSync } from 'node:fs';
import { randomUUID } from 'node:crypto';
import { _electron as electron } from '@playwright/test';
import { env, root } from './common.mjs';
Object.assign(process.env, env);
const output = path.join(root, 'tmp/packages', `Shellfox-${process.platform}-${process.arch}`);
const executablePath = process.platform === 'darwin' ? path.join(output, 'Shellfox.app/Contents/MacOS/Shellfox') : path.join(output, process.platform === 'win32' ? 'Shellfox.exe' : 'shellfox');
const data = path.join(root, 'tmp/packaged-startup-smoke', randomUUID());
mkdirSync(data, { recursive: true });
const application = await electron.launch({ executablePath, args: ['--test-user-data', data, '--test-backend', 'real'], timeout: 30000, env: { ...env, SHELLFOX_TEST_MODE: '1', SHELLFOX_TEST_ROOT: root } });
try {
  const page = await application.firstWindow({ timeout: 15000 });
  const errors = [];
  page.on('pageerror', error => errors.push(error.message));
  await page.getByRole('button', { name: 'New session', exact: true }).waitFor({ timeout: 15000 });
  assert.equal(await page.title(), 'Shellfox');
  assert.equal(await application.evaluate(({ app }) => app.getName()), 'Shellfox');
  const profiles = await page.evaluate(() => window.shellfox.getTerminalProfiles());
  assert.equal(profiles.ok, true, JSON.stringify(profiles));
  assert.equal(profiles.value.lifetime, 'app-owned');
  assert.equal(profiles.value.shellSurvival, false);
  assert.ok(profiles.value.profiles.some(profile => profile.available && profile.environment === 'local' && profile.canTerminateDescendants === true));
  assert.equal(await page.evaluate(() => typeof window.require), 'undefined');
  assert.deepEqual(errors, []);
  console.log('Production packaged renderer/preload startup passed without legacy native resources. No shells created.');
} finally { await application.close(); }
