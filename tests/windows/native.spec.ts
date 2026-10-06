import { test, expect } from '@playwright/test';
import { execFile } from 'node:child_process';
import { promisify } from 'node:util';
import path from 'node:path';
import { writeFile } from 'node:fs/promises';
import { randomUUID } from 'node:crypto';
import { launch, scratch, snapshot, value, root } from '../fixtures/electron';
const exec = promisify(execFile);
function requireDesktop() {
  expect(process.platform, 'Windows x64 interactive desktop required').toBe('win32');
  expect(process.arch).toBe('x64');
  expect(process.env.SHELLFOX_WINDOWS_GUI, 'Set SHELLFOX_WINDOWS_GUI=1 to authorize only test-owned terminals. No Explorer/installer mutation.').toBe('1');
}
async function powershell(code: string) {
  const ps = path.join(process.env.SystemRoot!, 'System32/WindowsPowerShell/v1.0/powershell.exe');
  return exec(ps, ['-NoProfile', '-NonInteractive', '-EncodedCommand', Buffer.from("$ErrorActionPreference='Stop';" + code, 'utf16le').toString('base64')], { timeout: 30000, windowsHide: true, env: { ...process.env, TMP: path.join(root, 'tmp'), TEMP: path.join(root, 'tmp') } });
}
const literal = (s: string) => "'" + s.replaceAll("'", "''") + "'";

test('real Electron service places six registered shells, focuses, and reconciles restart without relaunch', async () => {
  requireDesktop();
  const dir = await scratch('windows-product'); const data = path.join(dir, 'data');
  let { app, page } = await launch(data, 'real'); let closed = false;
  let shells: any[] = [];
  try {
    const probe = (await snapshot(page)).probe;
    expect(probe.available, probe.reasons.join('; ')).toBe(true);
    expect(probe.capabilities.createWindow).toBe(true);
    expect(probe.capabilities.addTab).toBe(true);
    expect(probe.capabilities.focusWindow).toBe(true);
    expect(probe.capabilities.processTracking).toBe(true);
    for (let s = 0; s < 2; s++) {
      const session = value(await page.evaluate(input => window.shellfox.createSession(input), { cwd: dir, title: `Native ${s}`, requestId: randomUUID() }));
      await expect.poll(async () => (await snapshot(page)).sessions.find(x => x.id === session.id)?.canAddTab, { timeout: 25000 }).toBe(true);
      for (let t = 0; t < 2; t++) {
        value(await page.evaluate(sessionId => window.shellfox.addTab({ sessionId }), session.id));
        await expect.poll(async () => (await snapshot(page)).sessions.find(x => x.id === session.id)?.tabs.filter(t => t.lifecycle === 'open').length, { timeout: 25000 }).toBe(t + 2);
      }
    }
    await expect.poll(async () => (await snapshot(page)).sessions.every(s => s.status === 'waiting'), { timeout: 15000 }).toBe(true);
    const readShells = () => app.evaluate(() => {
      const r = (globalThis as any).__shellfoxTest.repository;
      return r.tabs().filter((t: any) => t.registration).map((t: any) => ({ registration: t.registration, target: r.session(t.sessionId).target }));
    });
    shells = await readShells();
    expect(shells.length).toBe(6);
    expect(new Set(shells.map(s => s.registration.shell.pid)).size).toBe(6);
    expect(new Set(shells.map(s => s.target.hwnd)).size).toBe(2);
    for (const session of (await snapshot(page)).sessions) {
      const focus = await page.evaluate(sessionId => window.shellfox.focusSession({ sessionId }), session.id);
      expect(focus.ok || focus.error.code === 'FOCUS_DENIED', JSON.stringify(focus)).toBe(true);
      if (!focus.ok) value(await page.evaluate(sessionId => window.shellfox.clearSessionError({ sessionId }), session.id));
    }
    const operationCount = await app.evaluate(() => (globalThis as any).__shellfoxTest.repository.db.prepare('SELECT count(*) AS n FROM operations').get().n);
    await app.close(); closed = true;
    ({ app, page } = await launch(data, 'real')); closed = false;
    await expect.poll(async () => (await snapshot(page)).sessions.every(s => s.status === 'waiting' && s.canFocus), { timeout: 15000 }).toBe(true);
    expect(await readShells()).toEqual(shells);
    expect(await app.evaluate(() => (globalThis as any).__shellfoxTest.repository.db.prepare('SELECT count(*) AS n FROM operations').get().n)).toBe(operationCount);
    expect((await snapshot(page)).sessions.flatMap(s => s.tabs).length).toBe(6);
    await writeFile(path.join(dir, 'result.json'), JSON.stringify({ shells, restart: 'same registered identities, no new operations', registry: 'not modified' }, null, 2));
  } finally {
    if (!closed) {
      // Recover only records created in this fresh test database, including partial launches.
      shells = await app.evaluate(() => {
        const r = (globalThis as any).__shellfoxTest.repository;
        return r.tabs().filter((t: any) => t.registration).map((t: any) => ({ registration: t.registration, target: r.session(t.sessionId).target }));
      });
      await app.close();
    }
    const report = path.join(dir, 'cleanup.json');
    await writeFile(report, JSON.stringify({ shells }, null, 2));
    for (const entry of shells) {
      const identity = entry.registration.shell;
      await powershell(`$p=Get-Process -Id ${identity.pid} -ErrorAction SilentlyContinue;if($p -and $p.StartTime.ToFileTimeUtc().ToString() -eq '${identity.startTime}'){if(@(Get-CimInstance Win32_Process -Filter 'ParentProcessId=${identity.pid}').Count -gt 0){throw 'Unexpected children; test shell left untouched'};Stop-Process -Id $p.Id}`);
    }
    if (shells.some(s => !s.target)) throw new Error('Test shell cleaned, but unverified test HWND cannot be closed safely. Manual inspection required: ' + report);
    await powershell(`& ${literal(path.join(root, 'native/close-gate-windows.ps1'))} -ReportPath ${literal(report)}`);
  }
});

test('native gate authenticates registration, tracks scoped agents, survives broker restart and cleans owned windows', async () => {
  requireDesktop();
  let result;
  try {
    result = await exec(process.execPath, ['native/gate-windows.mjs', '--run-gui'], { cwd: root, timeout: 240000, maxBuffer: 1024 * 1024, env: { ...process.env, TEMP: path.join(root, 'tmp'), TMP: path.join(root, 'tmp') } });
  } catch (error) {
    // Do not mask the gate failure. Safely recover only its authenticated UUID tabs.
    await powershell(`& ${literal(path.join(root, 'tests/windows/cleanup-gate.ps1'))} -ReportPath ${literal(path.join(root, 'tmp/native-gate/result.json'))}`);
    throw error;
  }
  expect(result.stdout).toContain('authenticated-placement-tracking-proven');
  expect(result.stdout).toContain('"assertions":8');
});
