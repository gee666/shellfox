import { test, expect, type ElectronApplication } from '@playwright/test';
import { execFile, spawn, type ChildProcess } from 'node:child_process';
import { promisify } from 'node:util';
import { existsSync } from 'node:fs';
import { readFile } from 'node:fs/promises';
import path from 'node:path';
import { z } from 'zod';
import { hostStatusSchema } from '../../src/shared/host-schemas';
import { resultSchema, sessionSchema, snapshotSchema, terminalAttachmentSchema } from '../../src/shared/schemas';
import { launch, root, scratch, snapshot, value, env } from '../fixtures/electron';

const directory = process.env.SHELLFOX_HEADLESS_PACKAGE ?? path.join(root, 'tmp/headless', `shellfox-headless-${process.platform}-${process.arch}`);
const node = path.join(directory, 'bin', process.platform === 'win32' ? 'node.exe' : 'node');
const main = path.join(directory, 'app/main.cjs');
const headlessEnv = () => ({ ...env(), SHELLFOX_HEADLESS: '1', SHELLFOX_RESOURCES_PATH: path.join(directory, 'resources'), SHELLFOX_RUNTIME_PATH: directory });
async function cli<T>(data: string, schema: z.ZodType<T>, args: string[]): Promise<T> {
  const result = await promisify(execFile)(node, [main, '--user-data', data, ...args, '--json'], { cwd: root, env: headlessEnv(), windowsHide: true, timeout: 30_000 });
  return value(resultSchema(schema).parse(JSON.parse(result.stdout)));
}
async function start(data: string): Promise<ChildProcess> {
  const child = spawn(node, [main, '--user-data', data, 'host', 'run', '--json'], { cwd: root, env: headlessEnv(), stdio: ['ignore', 'pipe', 'pipe'], windowsHide: true });
  await new Promise<void>((resolve, reject) => {
    let stdout = '', stderr = '';
    const timer = setTimeout(() => { child.kill(); reject(new Error('Headless host startup timed out.')); }, 30_000);
    child.stderr?.on('data', chunk => { stderr += String(chunk); });
    child.once('error', error => { clearTimeout(timer); reject(error); });
    child.once('exit', code => { clearTimeout(timer); reject(new Error('Headless host exited during startup: ' + code + ' ' + stderr)); });
    child.stdout?.on('data', chunk => {
      stdout += String(chunk); if (!stdout.includes('\n')) return;
      try { const result = resultSchema(hostStatusSchema).parse(JSON.parse(stdout.split('\n')[0]!)); value(result); clearTimeout(timer); resolve(); }
      catch (error) { clearTimeout(timer); reject(error); }
    });
  });
  return child;
}
async function stop(data: string, child: ChildProcess) {
  await cli(data, z.object({ stopped: z.literal(true) }), ['host', 'stop', '--yes']).catch(() => {});
  if (child.exitCode === null && child.signalCode === null) await new Promise<void>(resolve => {
    const timer = setTimeout(() => { child.kill(); resolve(); }, 10_000);
    child.once('exit', () => { clearTimeout(timer); resolve(); });
  });
}

test.describe('desktop and Electron-free CLI use one host', () => {
  test.skip(!existsSync(main) || !existsSync(node), 'Run pnpm build:headless before this integration suite.');
  test.setTimeout(180_000);
  test('CLI JSON automation operates on the desktop host, including explicit shutdown', async () => {
    const work = await scratch('desktop-cli-host'), data = path.join(work, 'data');
    let application: ElectronApplication | undefined;
    try {
      const running = await launch(data, 'real'); application = running.app;
      const status = await cli(data, hostStatusSchema, ['host', 'status']);
      expect(status.mode).toBe('desktop');
      expect(status.pid).toBe(await application.evaluate(() => process.pid));
      const created = await cli(data, sessionSchema, ['sessions', 'create', work, '--title', 'Created through CLI']);
      await expect(running.page.getByRole('button', { name: 'Select session Created through CLI' })).toBeVisible();
      expect((await snapshot(running.page)).sessions.some(session => session.id === created.id)).toBe(true);
      await cli(data, z.unknown(), ['settings', 'update', '--input', JSON.stringify({ accentColor: '#0ea5e9' })]);
      await expect.poll(async () => (await snapshot(running.page)).settings.accentColor).toBe('#0ea5e9');
      const reading = await cli(data, snapshotSchema, ['api', 'getSnapshot']);
      expect(reading.sessions.find(session => session.id === created.id)?.tabs[0]?.generation).toBe(created.tabs[0]?.generation);
      const exited = application.waitForEvent('close', { timeout: 30_000 });
      await cli(data, z.object({ stopped: z.literal(true) }), ['host', 'stop', '--yes']);
      await exited; application = undefined;
    } finally { await application?.close().catch(() => {}); }
  });
  test('closing an attached desktop does not stop a headless host or its running command', async () => {
    const work = await scratch('headless-desktop-client'), data = path.join(work, 'data');
    const child = await start(data); let application: ElectronApplication | undefined;
    try {
      const status = await cli(data, hostStatusSchema, ['host', 'status']); expect(status.mode).toBe('headless');
      const created = await cli(data, sessionSchema, ['sessions', 'create', work, '--title', 'Server session']);
      const tab = created.tabs[0]; if (!tab?.generation) throw new Error('Host terminal missing.');
      const running = await launch(data, 'real'); application = running.app;
      expect(await application.evaluate(({ BrowserWindow }) => BrowserWindow.getAllWindows()[0]!.getTitle())).toContain('host client');
      expect(await application.evaluate(() => Reflect.has(globalThis, '__shellfoxTest'))).toBe(false);
      await expect(running.page.getByRole('button', { name: 'Select session Server session' })).toBeVisible();
      expect((await snapshot(running.page)).sessions.find(session => session.id === created.id)?.tabs[0]?.generation).toBe(tab.generation);
      const progress = path.join(work, 'progress.txt');
      const command = process.platform === 'win32'
        ? `1..40 | ForEach-Object { Add-Content -LiteralPath '${progress.replaceAll("'", "''")}' -Value $_; Start-Sleep -Milliseconds 100 }\r`
        : `for i in $(seq 1 40); do printf '%s\\n' "$i" >> '${progress.replaceAll("'", "'\\''")}'; sleep 0.1; done\r`;
      await cli(data, z.unknown(), ['terminals', 'write', tab.id, command]);
      await expect.poll(async () => (await readFile(progress, 'utf8').catch(() => '')).trim().length).toBeGreaterThan(0);
      await application.close(); application = undefined;
      expect((await cli(data, hostStatusSchema, ['host', 'status'])).pid).toBe(status.pid);
      await expect.poll(async () => (await readFile(progress, 'utf8').catch(() => '')).trim().split(/\r?\n/).filter(Boolean).length).toBe(40);
      const attachment = await cli(data, terminalAttachmentSchema, ['api', 'attachTerminal', '--input', JSON.stringify({ tabId: tab.id, generation: tab.generation })]);
      expect(attachment.state).toBe('open'); expect(attachment.generation).toBe(tab.generation);
    } finally { await application?.close().catch(() => {}); await stop(data, child); }
  });
});
