import { test, expect } from '@playwright/test';
import { randomUUID } from 'node:crypto';
import { existsSync } from 'node:fs';
import { writeFile } from 'node:fs/promises';
import path from 'node:path';
import { launch, scratch, snapshot, value } from '../fixtures/electron';

test('two tabs in each of eight real sessions keep Pi detection under concurrent load', async () => {
  test.skip(process.platform !== 'win32' || !existsSync(path.join(process.env.USERPROFILE ?? '', '.pi/agent/bin/pi.exe')));
  test.setTimeout(150000);
  const dir = await scratch('pi-load'), { app, page } = await launch(path.join(dir, 'data'), 'real');
  const evidence: unknown[] = [];
  try {
    const initial = await snapshot(page);
    value(await page.evaluate(settings => window.shellfox.saveSettings({ ...settings, terminalProfileId: 'pwsh' }), initial.settings));
    await app.evaluate(() => {
      const tracker = (globalThis as any).__shellfoxTest.service.tracker, provider = tracker.snapshotProvider;
      (globalThis as any).loadTrace = [];
      tracker.snapshotProvider = async (...args: any[]) => {
        const start = Date.now(), result = await provider(...args);
        (globalThis as any).loadTrace.push({ duration: Date.now() - start, roots: args[0].length, snapshots: result.map((s: any) => ({ complete: s.complete, reason: s.reason, rows: s.processes.length })) });
        return result;
      };
    });
    for (let n = 0; n < 8; n++) {
      const session = value(await page.evaluate(input => window.shellfox.createSession(input), { cwd: dir, requestId: randomUUID(), title: `Concurrent Pi ${n}` }));
      const added = value(await page.evaluate(sessionId => window.shellfox.addTab({ sessionId }), session.id));
      for (const tab of added.tabs) value(await page.evaluate(input => window.shellfox.writeTerminal!(input), { tabId: tab.id, generation: tab.generation!, data: 'pi\r' }));
    }
    const start = Date.now();
    for (let i = 0; i < 20; i++) { await page.waitForTimeout(1000); evidence.push({ ms: Date.now() - start, sessions: (await snapshot(page)).sessions }); }
    const tabs = (await snapshot(page)).sessions.flatMap(s => s.tabs);
    expect(tabs.map(t => t.status)).toEqual(Array(16).fill('running'));
    expect(tabs.every(t => t.agents > 0)).toBe(true);
  } finally {
    await writeFile(path.join(dir, 'evidence.json'), JSON.stringify({ evidence, trace: await app.evaluate(() => (globalThis as any).loadTrace) }, null, 2));
    console.log('Pi load evidence', path.join(dir, 'evidence.json'));
    await page.evaluate(async () => { const result = await window.shellfox.getSnapshot(); if (result.ok) for (const s of result.value.sessions) for (const t of s.tabs) if (t.terminalKind === 'embedded' && t.lifecycle !== 'closed') await window.shellfox.closeTab!({ tabId: t.id, generation: t.generation! }); }).catch(() => {});
    await app.close();
  }
});
