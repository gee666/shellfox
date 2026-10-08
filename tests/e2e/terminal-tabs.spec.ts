import { test, expect } from '@playwright/test';
import { randomUUID } from 'node:crypto';
import path from 'node:path';
import { readFileSync } from 'node:fs';
import { launch, scratch, snapshot, value } from '../fixtures/electron';

test('real terminal tab rename and drag order preserve shells and survive restart', async () => {
  const dir = await scratch('terminal-tabs'), data = path.join(dir, 'data');
  let running = await launch(data, 'real');
  try {
    const version = JSON.parse(readFileSync(path.resolve('package.json'), 'utf8')).version;
    await expect(running.page).toHaveTitle(`Shellfox v${version}`);
    expect(await running.app.evaluate(({ BrowserWindow }) => BrowserWindow.getAllWindows()[0]!.getTitle())).toBe(`Shellfox v${version}`);
    const created = value(await running.page.evaluate(input => window.shellfox.createSession(input), { cwd: dir, requestId: randomUUID(), title: 'Tab metadata' }));
    const second = value(await running.page.evaluate(input => window.shellfox.addTab(input), { sessionId: created.id }));
    const third = value(await running.page.evaluate(input => window.shellfox.addTab(input), { sessionId: created.id }));
    expect(third.tabs.every(tab => tab.lifecycle === 'open')).toBe(true);
    const [a, b, c] = third.tabs;
    const shellsBefore = await running.app.evaluate(() => (globalThis as any).__shellfoxTest.backend.live().map((entry: any) => [entry.tabId, entry.generation, entry.root.pid]));
    const tabB = running.page.locator(`#terminal-tab-${b.id}`);
    await tabB.dblclick();
    const name = running.page.getByRole('textbox', { name: 'Terminal tab name' });
    await name.fill('Tests 雪'); await name.press('Enter');
    await expect(tabB).toContainText('Tests 雪');
    await expect(tabB).toHaveAttribute('aria-selected', 'true');
    await running.page.locator(`#terminal-tab-${c.id}`).dragTo(running.page.locator('.terminal-tab-item').first());
    await expect.poll(async () => (await snapshot(running.page)).sessions.find(session => session.id === created.id)!.tabs.map(tab => tab.id)).toEqual([c.id, a.id, b.id]);
    await expect(tabB).toHaveAttribute('aria-selected', 'true');
    expect(await running.app.evaluate(() => (globalThis as any).__shellfoxTest.backend.live().map((entry: any) => [entry.tabId, entry.generation, entry.root.pid]))).toEqual(shellsBefore);
    const saved = (await snapshot(running.page)).sessions.find(session => session.id === created.id)!;
    expect(saved.tabs.map(tab => tab.generation)).toEqual([c.generation, a.generation, b.generation]);
    // Close only this test's shells before restarting; restarting never revives commands.
    for (const tab of saved.tabs) value(await running.page.evaluate(input => window.shellfox.closeTab!(input), { tabId: tab.id, generation: tab.generation! }));
    await running.app.close(); running = await launch(data, 'real');
    const restored = (await snapshot(running.page)).sessions.find(session => session.id === created.id)!;
    expect(restored.tabs.map(tab => [tab.id, tab.title, tab.ordinal, tab.lifecycle])).toEqual([[c.id, c.title, 0, 'closed'], [a.id, a.title, 1, 'closed'], [b.id, 'Tests 雪', 2, 'closed']]);
    expect(second.tabs).toHaveLength(2);
  } finally {
    await running.page.evaluate(async () => {
      const state = await window.shellfox.getSnapshot();
      if (state.ok) for (const session of state.value.sessions) for (const tab of session.tabs) {
        if (tab.terminalKind === 'embedded' && tab.lifecycle !== 'closed') await window.shellfox.closeTab!({ tabId: tab.id, generation: tab.generation! });
      }
    }).catch(() => {});
    await running.app.close();
  }
});
