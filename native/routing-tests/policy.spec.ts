import { test, expect } from '@playwright/test';
import { execFile } from 'node:child_process';
import { promisify } from 'node:util';
import { randomUUID } from 'node:crypto';
import path from 'node:path';
import { writeFile } from 'node:fs/promises';
import { launch, root, scratch, snapshot, value } from '../../tests/fixtures/electron';
const exec = promisify(execFile);

test('real service refuses append before creating an intent and preserves initial roots on restart', async () => {
  expect(process.env.SHELLFOX_WINDOWS_GUI).toBe('1');
  expect(process.platform).toBe('win32'); expect(process.arch).toBe('x64');
  const dir = await scratch('routing-policy-product'), data = path.join(dir, 'data');
  let { app, page } = await launch(data, 'real'); let closed = false;
  let shells: any[] = [];
  const registrations = () => app.evaluate(() => {
    const r = (globalThis as any).__shellfoxTest.repository;
    return r.tabs().filter((t: any) => t.registration).map((t: any) => ({ registration: t.registration, target: r.session(t.sessionId).target }));
  });
  const operations = () => app.evaluate(() => (globalThis as any).__shellfoxTest.repository.db.prepare('SELECT count(*) AS n FROM operations').get().n);
  try {
    const probe = (await snapshot(page)).probe;
    expect(probe.available).toBe(true); expect(probe.capabilities.addTab).toBe(false);
    for (let s = 0; s < 2; s++) {
      const session = value(await page.evaluate(input => window.shellfox.createSession(input), { cwd: dir, requestId: randomUUID() }));
      await expect.poll(async () => (await snapshot(page)).sessions.find(x => x.id === session.id)?.tabs[0]?.lifecycle, { timeout: 25000 }).toBe('open');
    }
    shells = await registrations(); expect(shells).toHaveLength(2);
    expect(new Set(shells.map(s => s.target.hwnd)).size).toBe(2);
    const before = await operations();
    for (const session of (await snapshot(page)).sessions) {
      expect(session.canAddTab).toBe(false);
      const append = await page.evaluate(sessionId => window.shellfox.addTab({ sessionId }), session.id);
      expect(append).toMatchObject({ ok: false, error: { code: 'UNSUPPORTED' } });
      const focus = await page.evaluate(sessionId => window.shellfox.focusSession({ sessionId }), session.id);
      expect(focus.ok || focus.error.code === 'FOCUS_DENIED').toBe(true);
    }
    expect(await operations()).toBe(before); expect(await registrations()).toEqual(shells);
    expect((await snapshot(page)).sessions.flatMap(s => s.tabs)).toHaveLength(2);
    await app.close(); closed = true;
    ({ app, page } = await launch(data, 'real')); closed = false;
    await expect.poll(async () => (await snapshot(page)).sessions.every(s => s.canFocus && !s.canAddTab && s.tabs[0].lifecycle === 'open'), { timeout: 15000 }).toBe(true);
    expect(await registrations()).toEqual(shells); expect(await operations()).toBe(before);
    await writeFile(path.join(dir, 'result.json'), JSON.stringify({ shells, operations: before, result: 'append refused before intent; same initial registrations after restart', multitabAcceptance: false }, null, 2));
  } finally {
    if (!closed) { shells = await registrations(); await app.close(); }
    const report = path.join(dir, 'cleanup.json');
    await writeFile(report, JSON.stringify({ shells, cleanup: shells.map(s => ({ shell: s.registration.shell })) }, null, 2));
    if (shells.length) {
      const ps = path.join(process.env.SystemRoot!, 'System32/WindowsPowerShell/v1.0/powershell.exe');
      const script = path.join(root, 'tests/windows/cleanup-gate.ps1');
      await exec(ps, ['-NoProfile', '-NonInteractive', '-File', script, '-ReportPath', report], { timeout: 30000, windowsHide: true, env: { ...process.env, TMP: path.join(root, 'tmp'), TEMP: path.join(root, 'tmp') } });
    }
  }
});
