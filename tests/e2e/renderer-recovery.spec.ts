import { test, expect, type ElectronApplication } from '@playwright/test';
import { randomUUID } from 'node:crypto';
import { readFile, rm } from 'node:fs/promises';
import path from 'node:path';
import { responseSchemas } from '../../src/shared/schemas';
import { calls, launch, scratch, seedTerminal, value } from '../fixtures/electron';

// Playwright marks its Page as crashed permanently. Read the replacement document through Electron.
const readDocument = (app: ElectronApplication, script: string): Promise<unknown> => app.evaluate(({ BrowserWindow }, source) => {
  const window = BrowserWindow.getAllWindows()[0];
  if (!window) throw new Error('Desktop window missing');
  return window.webContents.executeJavaScript(source);
}, script);
const crash = (app: ElectronApplication) => app.evaluate(({ BrowserWindow }) => new Promise<void>(resolve => {
  const window = BrowserWindow.getAllWindows()[0];
  if (!window) throw new Error('Desktop window missing');
  window.webContents.once('did-finish-load', () => resolve());
  window.webContents.forcefullyCrashRenderer();
}));
const expectWorkspace = async (app: ElectronApplication) => {
  await expect.poll(() => readDocument(app, `!!document.querySelector('main[aria-label="Workspace"]')`)).toBe(true);
};

test('repeated renderer crashes offer retry without relaunching sessions', async () => {
  const dir = await scratch('renderer-recovery');
  const { app, page } = await launch(path.join(dir, 'data'));
  try {
    const session = value(await page.evaluate(input => window.shellfox.createSession(input), {
      cwd: dir, title: 'Survives renderer crashes', requestId: randomUUID(),
    }));
    const launches = (await calls(app)).launches;
    for (let attempt = 0; attempt < 4; attempt++) {
      await crash(app);
      if (attempt < 3) await expectWorkspace(app);
    }
    expect(await readDocument(app, 'document.querySelector("h1")?.textContent')).toBe("Shellfox's display stopped");
    expect(await readDocument(app, 'document.querySelector("a")?.textContent')).toBe('Retry');
    await readDocument(app, 'document.querySelector("a").click()');
    await expectWorkspace(app);
    const restoredSnapshot = value(responseSchemas.getSnapshot.parse(await readDocument(app, 'window.shellfox.getSnapshot()')));
    const restored = restoredSnapshot.sessions.find(item => item.id === session.id);
    expect(restored?.tabs.map(tab => [tab.id, tab.generation])).toEqual(session.tabs.map(tab => [tab.id, tab.generation]));
    expect((await calls(app)).launches).toBe(launches);
    // Manual retry starts a fresh automatic recovery allowance.
    await crash(app);
    await expectWorkspace(app);
  } finally {
    await app.close();
    await rm(dir, { recursive: true, force: true });
  }
});

test('a real shell retains its variables and accepts input after manual renderer recovery', async () => {
  const dir = await scratch('renderer-recovery-pty');
  const { app, page } = await launch(path.join(dir, 'data'), 'real');
  try {
    const session = value(await page.evaluate(input => window.shellfox.createSession(input), {
      cwd: dir, title: 'Persistent shell', requestId: randomUUID(),
    }));
    const tab = session.tabs[0], token = randomUUID();
    if (!tab.generation) throw new Error('Terminal generation missing');
    const powershell = ['pwsh', 'windows-powershell'].includes(tab.profileId ?? '');
    const ready = path.join(dir, 'ready.txt'), restored = path.join(dir, 'restored.txt');
    const quote = (text: string) => `'${text.replaceAll("'", powershell ? "''" : "'\\''")}'`;
    const command = (destination: string, initialize: boolean) => powershell
      ? `${initialize ? `$shellfoxRecoveryToken=${quote(token)}; ` : ''}[IO.File]::WriteAllText(${quote(destination)}, $shellfoxRecoveryToken)\r`
      : `${initialize ? `SHELLFOX_RECOVERY_TOKEN=${quote(token)}; ` : ''}printf '%s' "$SHELLFOX_RECOVERY_TOKEN" > ${quote(destination)}\r`;
    value(await seedTerminal(app, { tabId: tab.id, generation: tab.generation, data: command(ready, true) }));
    await expect.poll(() => readFile(ready, 'utf8').catch(() => ''), { timeout: 15000 }).toBe(token);
    for (let attempt = 0; attempt < 4; attempt++) await crash(app);
    expect(await readDocument(app, 'document.querySelector("a")?.textContent')).toBe('Retry');
    await readDocument(app, 'document.querySelector("a").click()');
    await expectWorkspace(app);
    value(await seedTerminal(app, { tabId: tab.id, generation: tab.generation, data: command(restored, false) }));
    // The value exists only in the original shell's memory, not in launch settings.
    await expect.poll(() => readFile(restored, 'utf8').catch(() => ''), { timeout: 15000 }).toBe(token);
  } finally {
    await app.close();
    await rm(dir, { recursive: true, force: true });
  }
});
