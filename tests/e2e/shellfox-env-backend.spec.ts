import { test, expect } from '@playwright/test';
import { randomUUID } from 'node:crypto';
import { execFile } from 'node:child_process';
import { promisify } from 'node:util';
import path from 'node:path';
import { launch, scratch, snapshot, value } from '../fixtures/electron';
import type { TabDto } from '../../src/shared/contracts';

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

for (const profileId of ['pwsh', 'windows-powershell']) test(`${profileId} session env survives entering and leaving an interactive nested WSL shell`, async () => {
  test.skip(process.platform !== 'win32', 'Requires Windows shells and an already-running WSL distro.');
  test.setTimeout(90000);
  const run = promisify(execFile);
  let distro = '';
  try {
    const listed = await run('wsl.exe', ['--list', '--running', '--quiet'], { encoding: 'buffer', windowsHide: true, timeout: 8000 });
    distro = listed.stdout.toString(listed.stdout.includes(0) ? 'utf16le' : 'utf8').replace(/^\uFEFF/, '').split(/\r?\n/).map(s => s.trim()).find(Boolean) ?? '';
  } catch { /* No WSL transport available. */ }
  test.skip(!distro, 'Requires an already-running distro; this test never boots a stopped guest.');
  const dir = await scratch('nested-wsl-env'), { app, page } = await launch(path.join(dir, 'data'), 'real');
  const name = 'SHELLFOX_TEST_' + randomUUID().replaceAll('-', '').toUpperCase(), literal = 'literal $() ; C:\\folder = value';
  let tab: TabDto | undefined;
  const output = () => app.evaluate((_electron, id) => {
    const attached = (globalThis as any).__shellfoxTest.service.attachTerminal({ tabId: id });
    return attached.ok ? attached.value.chunks.map((chunk: any) => chunk.data).join('') : '';
  }, tab!.id);
  const write = async (data: string) => value(await page.evaluate(input => window.shellfox.writeTerminal!(input), { tabId: tab!.id, generation: tab!.generation!, data }));
  try {
    const profiles = value(await page.evaluate(() => window.shellfox.getTerminalProfiles!()));
    test.skip(!profiles.profiles.some(p => p.id === profileId && p.available), 'PowerShell profile unavailable.');
    const initial = await snapshot(page);
    value(await page.evaluate(settings => window.shellfox.saveSettings(settings), { ...initial.settings, terminalProfileId: profileId }));
    const session = value(await page.evaluate(input => window.shellfox.createSession(input), { cwd: dir, requestId: randomUUID() }));
    value(await page.evaluate(input => window.shellfox.setSessionEnv(input), { sessionId: session.id, env: [
      { name, value: literal }, { name: 'WSLENV', value: `${name}/pw:TERM/l` },
    ] }));
    const added = value(await page.evaluate(id => window.shellfox.addTab({ sessionId: id }), session.id));
    tab = added.tabs.at(-1)!;
    await write("[Console]::WriteLine('HOST_'+'READY')\r");
    await expect.poll(output, { timeout: 15000 }).toContain('HOST_READY');
    // Suppress guest dotfiles so arbitrary user startup commands cannot affect
    // the test. This is an interactive WSL shell inside the owned PowerShell PTY.
    await write(`wsl.exe --distribution '${distro.replaceAll("'", "''")}' --exec /bin/bash --noprofile --norc -i\r`);
    await expect.poll(output, { timeout: 15000 }).toMatch(/bash-[\d.]+\$ /);
    await write("printf 'GUEST_%s\\n' READY\r");
    await expect.poll(output, { timeout: 15000 }).toContain('GUEST_READY');
    await write(`python3 -c 'import os,json;print("NESTED_"+"ENV="+json.dumps(os.environ.get("${name}")))'\r`);
    await expect.poll(output, { timeout: 15000 }).toContain('NESTED_ENV=' + JSON.stringify(literal));
    const beforeExit = (await output()).length;
    await write('exit\r');
    await expect.poll(async () => (await output()).slice(beforeExit), { timeout: 15000 }).toContain('PS ');
    await write(`[Console]::WriteLine('HOST_'+'ENV=' + [Environment]::GetEnvironmentVariable('${name}'))\r`);
    await expect.poll(output, { timeout: 15000 }).toContain('HOST_ENV=' + literal);
  } finally {
    if (tab) await write('exit\r').catch(() => {});
    await page.evaluate(async () => {
      const state = await window.shellfox.getSnapshot();
      if (state.ok) for (const session of state.value.sessions) for (const item of session.tabs) if (item.lifecycle !== 'closed') await window.shellfox.closeTab!({ tabId: item.id, generation: item.generation! });
    }).catch(() => {});
    await app.close();
  }
});
