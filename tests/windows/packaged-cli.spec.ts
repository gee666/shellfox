import { test, expect } from '@playwright/test';
import path from 'node:path';
import { mkdir, writeFile } from 'node:fs/promises';
import { existsSync } from 'node:fs';
import { execFile } from 'node:child_process';
import { promisify } from 'node:util';
import { launch, scratch, snapshot, executable, root } from '../fixtures/electron';
import { cli } from '../fixtures/cli';
const exec = promisify(execFile);

test('production packaged CLI opens distinct quoted-path sessions cold/warm and survives restart', async () => {
  expect(process.platform).toBe('win32'); expect(process.arch).toBe('x64');
  expect(process.env.SHELLFOX_WINDOWS_GUI, 'Explicit terminal permission required').toBe('1');
  expect(existsSync(executable), 'Run pnpm package before Windows packaged CLI acceptance').toBe(true);
  const dir = await scratch('packaged-cli'); const data = path.join(dir, 'data');
  const cwd = path.join(dir, "same Ω ' & ; % [data]"); await mkdir(cwd);
  // Main-process debugger evaluation reads this isolated SQLite database only.
  // Production has no repository hook or renderer database method.
  let { app, page } = await launch(data, 'real', true, ['--new-session', '--cwd', cwd + '\\.']);
  let closed = false; let shells: any[] = [];
  const read = () => app.evaluate(({ app }) => {
    const require = process.getBuiltinModule('module').createRequire(app.getAppPath() + '/package.json');
    const Database = require(app.getAppPath() + '/node_modules/better-sqlite3');
    const db = new Database(app.getPath('userData') + '/manager.sqlite3', { readonly: true, fileMustExist: true });
    try {
      const rows = db.prepare('SELECT tabs.registration, sessions.target FROM tabs JOIN sessions ON tabs.sessionId=sessions.id WHERE tabs.registration IS NOT NULL').all();
      return { shells: rows.map((r: any) => ({ registration: JSON.parse(r.registration), target: JSON.parse(r.target) })), operations: db.prepare('SELECT count(*) AS n FROM operations').get().n };
    } finally { db.close(); }
  });
  try {
    await expect.poll(async () => (await snapshot(page)).sessions.filter(s => s.tabs[0].lifecycle === 'open').length, { timeout: 25000 }).toBe(1);
    for (let n = 0; n < 2; n++) {
      expect((await cli(data, ['--new-session', '--cwd', cwd + '\\.'], true)).code).toBe(0);
      await expect.poll(async () => (await snapshot(page)).sessions.filter(s => s.tabs[0].lifecycle === 'open').length, { timeout: 25000 }).toBe(n + 2);
    }
    const sessions = (await snapshot(page)).sessions;
    expect(new Set(sessions.map(s => s.id)).size).toBe(3);
    expect(sessions.every(s => s.cwd === cwd)).toBe(true);
    const before = await read(); shells = before.shells;
    expect(new Set(shells.map(s => s.target.hwnd)).size).toBe(3);
    await app.close(); closed = true;
    ({ app, page } = await launch(data, 'real', true)); closed = false;
    await expect.poll(async () => (await snapshot(page)).sessions.every(s => s.canFocus && s.status === 'waiting'), { timeout: 15000 }).toBe(true);
    expect(await read()).toEqual(before);
    expect((await snapshot(page)).sessions.map(s => s.id).sort()).toEqual(sessions.map(s => s.id).sort());
    await writeFile(path.join(dir, 'result.json'), JSON.stringify({ shells, operations: before.operations, explorer: 'argv shape only; no Explorer click or registration' }, null, 2));
  } finally {
    if (!closed) { try { shells = (await read()).shells; } finally { await app.close(); } }
    const report = path.join(dir, 'cleanup.json');
    await writeFile(report, JSON.stringify({ shells, cleanup: shells.map(s => ({ shell: s.registration.shell })) }, null, 2));
    const ps = path.join(process.env.SystemRoot!, 'System32/WindowsPowerShell/v1.0/powershell.exe');
    await exec(ps, ['-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass', '-File', path.join(root, 'tests/windows/cleanup-gate.ps1'), '-ReportPath', report], { timeout: 45000, windowsHide: true });
  }
});
