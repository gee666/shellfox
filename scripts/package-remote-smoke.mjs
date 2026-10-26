// Real packaged helper bootstrap. Contacts Tailscale without enrolling an account.
// Uses isolated project tmp data, never the user's node identity or terminals.
import assert from 'node:assert/strict';
import path from 'node:path';
import { mkdirSync } from 'node:fs';
import { randomUUID } from 'node:crypto';
import { _electron as electron } from '@playwright/test';
import { env, root } from './common.mjs';
Object.assign(process.env, env);
const output = path.join(root, 'tmp/packages', `Shellfox-${process.platform}-${process.arch}`);
const executablePath = process.platform === 'darwin' ? path.join(output, 'Shellfox.app/Contents/MacOS/Shellfox') : path.join(output, process.platform === 'win32' ? 'Shellfox.exe' : 'shellfox');
const data = path.join(root, 'tmp/packaged-remote-smoke', randomUUID());
mkdirSync(data, { recursive: true });
const application = await electron.launch({ executablePath, args: ['--test-user-data', data, '--test-backend', 'real'], timeout: 30000, env: { ...env, SHELLFOX_TEST_MODE: '1', SHELLFOX_TEST_ROOT: root } });
try {
  const page = await application.firstWindow({ timeout: 15000 });
  await page.waitForFunction(() => !!window.shellfoxRemote);
  const enabled = await page.evaluate(() => window.shellfoxRemote.configure({ enabled: true, hostname: 'shellfox-bootstrap-smoke' }));
  assert.equal(enabled.ok, true, 'Packaged embedded helper did not start: ' + (!enabled.ok ? enabled.error.message : ''));
  const deadline = Date.now() + 20000;
  let status;
  do {
    const state = await page.evaluate(() => window.shellfoxRemote.getState());
    assert.equal(state.ok, true, 'Packaged remote state failed');
    status = state.value.status;
    assert.notEqual(status, 'error', 'Packaged Tailscale bootstrap failed');
    assert.notEqual(status, 'online', 'Fresh smoke identity must not enroll an account');
    if (status === 'login-required') break;
    // A bounded condition wait, not a fixed delay or credentials-based login.
    await page.waitForFunction(async () => {
      const result = await window.shellfoxRemote.getState();
      return result.ok && result.value.status !== 'starting';
    }, undefined, { timeout: Math.max(1, deadline - Date.now()), polling: 250 });
  } while (Date.now() < deadline);
  assert.equal(status, 'login-required', 'Helper did not reach the sign-in state');
  const snapshot = await page.evaluate(() => window.shellfox.getSnapshot());
  assert.equal(snapshot.ok, true);
  assert.equal(snapshot.value.sessions.length, 0, 'Bootstrap smoke must not create terminals');
  const disabled = await page.evaluate(() => window.shellfoxRemote.configure({ enabled: false, hostname: 'shellfox-bootstrap-smoke' }));
  assert.equal(disabled.ok, true);
  assert.equal(disabled.value.status, 'disabled');
  console.log('Packaged embedded Tailscale bootstrap passed: sign-in required, no account enrollment, no terminals, clean disable.');
} finally { await application.close(); }
