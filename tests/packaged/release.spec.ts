import { test, expect } from '@playwright/test';
import path from 'node:path';
import { existsSync, readdirSync } from 'node:fs';
import { createRequire } from 'node:module';
import { launch, scratch, snapshot, executable, value } from '../fixtures/electron';
import { cli } from '../fixtures/cli';

const require = createRequire(path.resolve('package.json'));
test('production package excludes fixtures and test hooks; packaged SQLite and helper initialize', async () => {
  expect(existsSync(executable), 'Run pnpm package first').toBe(true);
  const resources = path.join(path.dirname(executable), 'resources');
  const { listPackage, extractFile } = require('@electron/asar');
  const archive = path.join(resources, 'app.asar');
  const resourceFiles = readdirSync(resources, { recursive: true }).map(String);
  expect(resourceFiles.filter(f => /(?:\.sqlite3(?:-|$)|launch-tickets|Shellfox\.Native\.Tests|fake-native|fixture|architecture|build-test|reports|playwright)/i.test(f))).toEqual([]);
  const files: string[] = listPackage(archive);
  expect(files.filter(f => /(?:tests|fixture|architecture|playwright|reports|verification|build-test|launch-tickets|\.pnpm)/i.test(f))).toEqual([]);
  const main = extractFile(archive, path.join('tmp', 'build', 'main', 'index.cjs')).toString();
  expect(main).not.toMatch(/__shellfoxTest|createFakeNativeBackend|FakeNativeBackend|SHELLFOX_FAKE/);
  expect(existsSync(path.join(resources, 'native/Shellfox.Native.exe'))).toBe(true);
  expect(existsSync(path.join(resources, 'native/coreclr.dll'))).toBe(true);
  expect(existsSync(path.join(resources, 'shell/bootstrap.ps1'))).toBe(true);
  expect(files.some(f => f.includes('better-sqlite3') && f.endsWith('.node'))).toBe(true);
  expect(existsSync(path.join(resources, 'app.asar.unpacked/node_modules/better-sqlite3/prebuilds/win32-x64.node'))).toBe(true);
  const dir = await scratch('packaged'); const data = path.join(dir, 'data');
  let { app, page } = await launch(data, 'real', true);
  let closed = false;
  try {
    const info = await app.evaluate(({ app }) => ({ packaged: app.isPackaged, userData: app.getPath('userData'), hook: typeof (globalThis as any).__shellfoxTest }));
    expect(info).toEqual({ packaged: true, userData: data, hook: 'undefined' });
    const initial = await snapshot(page);
    expect(initial.probe.available, initial.probe.reasons.join('; ')).toBe(true);
    expect(initial.probe.capabilities.createWindow).toBe(true);
    expect(initial.probe.capabilities.processTracking).toBe(true);
    expect(initial.probe.capabilities.activateTab).toBe(false);
    expect(initial.sessions).toEqual([]);
    const settings = { ...initial.settings, accentColor: '#8844cc' };
    value(await page.evaluate(settings => window.shellfox.saveSettings(settings), settings));
    expect((await cli(data, [], true)).code).toBe(0);
    expect((await snapshot(page)).sessions).toEqual([]);
    expect(await page.evaluate(() => typeof (window as any).require)).toBe('undefined');
    await app.close(); closed = true;
    ({ app, page } = await launch(data, 'real', true)); closed = false;
    expect((await snapshot(page)).settings.accentColor).toBe('#8844cc');
    expect((await snapshot(page)).probe.available).toBe(true);
    expect(existsSync(path.join(data, 'manager.sqlite3'))).toBe(true);
  } finally { if (!closed) await app.close(); }
});

test('packaged production rejects fake selection, unapproved test flags and unknown CLI', async () => {
  expect(existsSync(executable), 'Run pnpm package first').toBe(true);
  const dir = await scratch('packaged-reject');
  const fake = await cli(dir, [], true, {}, 'fake');
  expect(fake.code).toBe(1);
  expect(fake.stderr).toContain('Fake backend is not in the production build');
  const disabled = await cli(dir, [], true, { SHELLFOX_TEST_MODE: '0' });
  expect(disabled.code).toBe(1);
  expect(disabled.stderr).toContain('Test flags require explicit test mode');
  const bad = await cli(dir, ['--unknown'], true);
  expect(bad.code).toBe(1);
  expect(bad.stderr).toContain('Unknown product flag');
  expect(existsSync(path.join(dir, 'manager.sqlite3'))).toBe(false);
});
