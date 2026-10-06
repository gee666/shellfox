import { test, expect, _electron } from '@playwright/test';
import path from 'node:path';
import { mkdir } from 'node:fs/promises';
import { env, root, testMain, scratch, launch, snapshot, calls } from '../fixtures/electron';
import { cli } from '../fixtures/cli';

test('two cold-start CLI requests queue before renderer and warm calls do not deduplicate the folder', async () => {
  const dir = await scratch('cli');
  const data = path.join(dir, 'data');
  const cwd = path.join(dir, "quoted Ω ' & ; % [data]"); await mkdir(cwd);
  const app = await _electron.launch({ cwd: root, env: { ...env(), SHELLFOX_FAKE_INIT_DELAY_MS: '3000' }, args: [testMain, '--test-user-data', data, '--test-backend', 'fake', '--new-session', '--cwd', cwd] });
  try {
    // Lock acquired, but deliberately slow fake initialization has not made the renderer.
    await expect.poll(() => app.evaluate(() => !!(globalThis as any).__shellfoxTest)).toBe(true);
    expect(await app.evaluate(({ BrowserWindow }) => BrowserWindow.getAllWindows().length)).toBe(0);
    const second = await cli(data, ['--new-session', '--cwd', cwd]);
    expect(second.code).toBe(0);
    const page = await app.firstWindow(); await page.waitForFunction(() => !!window.shellfox);
    await expect.poll(async () => (await snapshot(page)).sessions.length).toBe(2);
    expect((await snapshot(page)).sessions.every(s => s.cwd === cwd)).toBe(true);
    const third = await cli(data, ['--new-session', '--cwd', cwd]); expect(third.code).toBe(0);
    await expect.poll(async () => (await snapshot(page)).sessions.length).toBe(3);
    expect((await calls(app)).launches).toBe(3);
    const show = await cli(data); expect(show.code).toBe(0);
    expect((await calls(app)).launches).toBe(3);
  } finally { await app.close(); }
});

test('isolated userData has separate lock and database; malformed CLI fails nonzero', async () => {
  const dir = await scratch('isolation');
  const first = await launch(path.join(dir, 'one'));
  const second = await launch(path.join(dir, 'two'));
  try {
    expect(await first.app.evaluate(({ app }) => app.getPath('userData'))).toBe(path.join(dir, 'one'));
    expect(await second.app.evaluate(({ app }) => app.getPath('userData'))).toBe(path.join(dir, 'two'));
    for (const args of [['--unknown'], ['--new-session'], ['--new-session', '--cwd', 'relative'], ['--new-session', '--cwd', '\\\\server\\share'], ['--new-session', '--cwd', dir, '--cwd', dir]]) {
      const invalid = await cli(path.join(dir, 'invalid'), args);
      expect(invalid.code).toBe(1);
      expect(invalid.stderr).toContain('Shellfox:');
    }
    const disabled = await cli(path.join(dir, 'invalid'), [], false, { SHELLFOX_TEST_MODE: '0' });
    expect(disabled.code).toBe(1);
    expect((await snapshot(first.page)).sessions.length).toBe(0);
    expect((await snapshot(second.page)).sessions.length).toBe(0);
  } finally { await second.app.close(); await first.app.close(); }
});
