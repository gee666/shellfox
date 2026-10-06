import { test, expect } from '@playwright/test';
import { randomUUID } from 'node:crypto';
import path from 'node:path';
import { launch, scratch, snapshot, value } from '../fixtures/electron';

test('session env survives SQLite restart and applies to new shells, not already running shells', async () => {
  test.skip(process.platform !== 'win32', 'Real PowerShell environment inheritance.');
  test.setTimeout(90000);
  const dir = await scratch('shellfox-env-backend'), data = path.join(dir, 'data'), name = 'SHELLFOX_TEST_' + randomUUID().replaceAll('-', '').toUpperCase();
  let current = await launch(data, 'real');
  async function output(tabId: string): Promise<string> {
    return current.app.evaluate((_electron, id) => new Promise<string>((resolve, reject) => setImmediate(() => {
      try {
        const result = (globalThis as any).__shellfoxTest.service.attachTerminal({ tabId: id });
        if (!result.ok) throw new Error(result.error.message);
        resolve(result.value.chunks.map((chunk: any) => chunk.data).join(''));
      } catch (error) { reject(error); }
    })), tabId);
  }
  async function closeTabs() {
    await current.page.evaluate(async () => {
      const result = await window.shellfox.getSnapshot();
      if (result.ok) for (const session of result.value.sessions) for (const tab of session.tabs) if (tab.terminalKind === 'embedded' && tab.lifecycle !== 'closed') await window.shellfox.closeTab!({ tabId: tab.id, generation: tab.generation! });
    });
  }
  try {
    const initial = await snapshot(current.page);
    value(await current.page.evaluate(settings => window.shellfox.saveSettings({ ...settings, terminalProfileId: 'pwsh' }), initial.settings));
    const session = value(await current.page.evaluate(input => window.shellfox.createSession(input), { cwd: dir, requestId: randomUUID() })), old = session.tabs[0];
    value(await current.page.evaluate(input => window.shellfox.setSessionEnv(input), { sessionId: session.id, env: [{ name: ' ' + name + ' ', value: 'round3 stored=with spaces' }] }));
    value(await current.page.evaluate(input => window.shellfox.writeTerminal!(input), { tabId: old.id, generation: old.generation!, data: `[Console]::WriteLine('SHELLFOX_BEFORE=' + [Environment]::GetEnvironmentVariable('${name}'))\r` }));
    await expect.poll(() => output(old.id), { timeout: 15000 }).toContain('SHELLFOX_BEFORE=\r\n');
    const added = value(await current.page.evaluate(id => window.shellfox.addTab({ sessionId: id }), session.id)), next = added.tabs.at(-1)!;
    value(await current.page.evaluate(input => window.shellfox.writeTerminal!(input), { tabId: next.id, generation: next.generation!, data: `[Console]::WriteLine('SHELLFOX_AFTER=' + [Environment]::GetEnvironmentVariable('${name}'))\r` }));
    await expect.poll(() => output(next.id), { timeout: 15000 }).toContain('SHELLFOX_AFTER=round3 stored=with spaces');
    await closeTabs(); await current.app.close(); current = await launch(data, 'real');
    expect((await snapshot(current.page)).sessions[0].env).toEqual([{ name, value: 'round3 stored=with spaces' }]);
  } finally { await closeTabs().catch(() => {}); await current.app.close(); }
});
