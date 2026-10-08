import { test, expect } from '@playwright/test';
import { randomUUID } from 'node:crypto';
import { existsSync } from 'node:fs';
import path from 'node:path';
import { launch, scratch, snapshot, value } from '../fixtures/electron';

test('embedded startup archives legacy-only tasks, upgrades untouched rules and leaves external roots untouched', async () => {
  const dir = await scratch('legacy-archive'), data = path.join(dir, 'data'), sessionId = randomUUID(), tabId = randomUUID();
  let running = await launch(data, 'real');
  try {
    const now = new Date().toISOString();
    await running.app.evaluate((_electron, input) => new Promise<void>((resolve, reject) => setImmediate(() => {
      try {
        const repository = (globalThis as any).__shellfoxTest.repository;
        repository.saveSession({ id: input.sessionId, title: 'learn-language', cwd: input.dir, adapterId: 'windows-terminal', shellId: 'pwsh', shellExecutable: 'C:\\PowerShell\\pwsh.exe', createdAt: input.now, updatedAt: input.now, settledAt: null, error: null, target: null });
        repository.saveTab({ id: input.tabId, sessionId: input.sessionId, title: 'External shell', cwd: input.dir, ordinal: 0, createdAt: input.now, lifecycle: 'open', operationId: input.operationId, error: null,
          registration: { sessionId: input.sessionId, tabId: input.tabId, operationId: input.operationId, shell: { pid: 777, startTime: '123456' }, shellExecutable: 'C:\\PowerShell\\pwsh.exe', cwd: input.dir, registeredAt: input.now } });
        const settings = repository.settings();
        settings.processRules = settings.processRules.map(({ processNames: _, ...rule }: any) => ({ ...rule, label: rule.label + ' Node launcher', executableBasenames: ['node.exe', 'node'],
          ...(rule.label === 'Pi' ? { enabled: false, scriptPathSuffixes: ['@earendil-works/pi-coding-agent/dist/cli.js', '@mariozechner/pi-coding-agent/dist/cli.js'] } : {}) }));
        settings.processRules.push({ id: 'f919fb1a-fb03-4a93-8b9b-1cde465d5873', label: 'Native agents', enabled: true,
          executableBasenames: ['claude.exe', 'claude', 'codex.exe', 'codex', 'opencode.exe', 'opencode'], executablePaths: [], scriptPathSuffixes: [] });
        repository.saveSettings(settings);
        resolve();
      } catch (error) { reject(error); }
    })), { sessionId, tabId, dir, now, operationId: randomUUID() });
    await running.app.close();
    running = await launch(data, 'real');
    expect((await snapshot(running.page)).sessions).toHaveLength(0);
    const history = value(await running.page.evaluate(() => window.shellfox.getHistory({ search: '', status: 'all', page: 1, pageSize: 20 })));
    expect(history.items[0]).toMatchObject({ id: sessionId, status: 'settled', tabs: [{ lifecycle: 'open', terminalKind: 'external-legacy', agents: 0 }] });
    const settings = (await snapshot(running.page)).settings;
    expect(settings.processRules.find(rule => rule.label === 'Pi')).toMatchObject({ enabled: false, processNames: ['pi', 'pi.exe'] });
    expect(settings.processRules.find(rule => rule.label === 'Pi')?.scriptPathSuffixes).toContain('@earendil-works/pi-coding-agent/dist/bundle/cli.js');
    expect(settings.processRules.some(rule => rule.label === 'Native agents')).toBe(false);
    await running.page.getByRole('button', { name: 'Archived · 1' }).click();
    await running.page.getByRole('button', { name: 'Select session learn-language' }).click();
    await expect(running.page.getByText('Archived session', { exact: true })).toBeVisible();
    await expect(running.page.getByRole('button', { name: 'Restore', exact: true })).toHaveCount(0);
    await expect(running.page.getByRole('button', { name: 'Focus', exact: true })).toHaveCount(0);
    await expect(running.page.getByLabel('Interactive terminal')).toHaveCount(0);
    const savedTab = await running.app.evaluate((_electron, id) => new Promise<any>((resolve, reject) => setImmediate(() => {
      try { resolve((globalThis as any).__shellfoxTest.repository.tab(id)); } catch (error) { reject(error); }
    })), tabId);
    expect(savedTab.registration.shell).toEqual({ pid: 777, startTime: '123456' });
    expect(savedTab.lifecycle).toBe('open');
  } finally { await running.app.close(); }
});

test('real PowerShell 7 Pi shim becomes running/green and output produces busy then idle transitions', async () => {
  test.skip(process.platform !== 'win32' || !existsSync(path.join(process.env.USERPROFILE ?? '', '.pi/agent/bin/pi.exe')), 'Requires the installed Windows Pi shim and PowerShell 7.');
  test.setTimeout(90000);
  const dir = await scratch('pi-tracking'), { app, page } = await launch(path.join(dir, 'data'), 'real');
  try {
    const initial = await snapshot(page);
    value(await page.evaluate(settings => window.shellfox.saveSettings({ ...settings, terminalProfileId: 'pwsh' }), initial.settings));
    const session = value(await page.evaluate(input => window.shellfox.createSession(input), { cwd: dir, requestId: randomUUID(), title: 'Pi detection' }));
    const tab = session.tabs[0];
    await expect.poll(async () => (await snapshot(page)).sessions[0].tabs[0].status, { timeout: 20000 }).toBe('waiting');
    await expect(page.getByRole('tab').locator('.status-dot')).toHaveAttribute('aria-label', 'Shell');
    await page.evaluate(id => {
      (window as any).acceptanceActivity = [];
      window.shellfox.subscribeTerminal!(event => { if (event.type === 'activity' && event.tabId === id) (window as any).acceptanceActivity.push(event); });
    }, tab.id);
    value(await page.evaluate(input => window.shellfox.writeTerminal!(input), { tabId: tab.id, generation: tab.generation!, data: 'pi\r' }));
    await expect.poll(async () => (await snapshot(page)).sessions[0].tabs[0].status, { timeout: 25000 }).toBe('running');
    expect((await snapshot(page)).sessions[0].tabs[0].agents).toBeGreaterThan(0);
    await expect.poll(async () => page.evaluate(() => (window as any).acceptanceActivity.some((event: any) => event.busy)), { timeout: 20000 }).toBe(true);
    await expect(page.getByRole('tab').locator('.status-dot')).toHaveAttribute('aria-label', /Agent (working|idle)/);
    await expect.poll(async () => page.evaluate(() => {
      const events = (window as any).acceptanceActivity, busyAt = events.findIndex((event: any) => event.busy);
      return busyAt >= 0 && events.slice(busyAt + 1).some((event: any) => !event.busy);
    }), { timeout: 20000 }).toBe(true);
    await expect(page.getByRole('tab').locator('.status-dot')).toHaveClass(/dot-(running|busy)/);
    const ended = value(await page.evaluate(input => window.shellfox.closeTab!(input), { tabId: tab.id, generation: tab.generation! }));
    expect(ended.tabs[0].lifecycle).toBe('closed');
    await expect.poll(async () => (await snapshot(page)).sessions[0].tabs[0].lifecycle).toBe('closed');
  } finally {
    // Close only this test's owned tabs, never the engineer/user's live Pi tree.
    await page.evaluate(async () => { const result = await window.shellfox.getSnapshot(); if (result.ok) for (const s of result.value.sessions) for (const t of s.tabs) if (t.terminalKind === 'embedded' && t.lifecycle !== 'closed') await window.shellfox.closeTab!({ tabId: t.id, generation: t.generation! }); }).catch(() => {});
    await app.close();
  }
});
