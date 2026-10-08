import { test, expect } from '@playwright/test';
import { randomUUID } from 'node:crypto';
import { existsSync } from 'node:fs';
import { writeFile } from 'node:fs/promises';
import path from 'node:path';
import { launch, scratch, snapshot, value } from '../fixtures/electron';

test('second real PowerShell tab detects idle Pi ten times', async () => {
  test.skip(process.platform !== 'win32' || !existsSync(path.join(process.env.USERPROFILE ?? '', '.pi/agent/bin/pi.exe')));
  test.setTimeout(180000);
  const dir = await scratch('second-pi'), { app, page } = await launch(path.join(dir, 'data'), 'real', false, ['start', dir]);
  const evidence: unknown[] = [];
  try {
    await expect.poll(async () => (await snapshot(page)).sessions.length, { timeout: 15000 }).toBe(1);
    const initial = await snapshot(page);
    value(await page.evaluate(settings => window.shellfox.saveSettings({ ...settings, terminalProfileId: 'pwsh' }), initial.settings));
    await app.evaluate(() => {
      const tracker = (globalThis as any).__shellfoxTest.service.tracker;
      const provider = tracker.snapshotProvider;
      (globalThis as any).trackingTrace = [];
      tracker.snapshotProvider = async (...args: any[]) => {
        const start = Date.now(), result = await provider(...args);
        const completedAt = Date.now();
        (globalThis as any).trackingTrace.push({ startedAt: start, completedAt, duration: completedAt - start, roots: args[0], snapshots: result.map((s: any) => ({ ...s, processes: s.processes.filter((p: any) => p.accessible || args[0].some((r: any) => r.pid === p.pid)) })) });
        return result;
      };
    });
    for (let n = 0; n < 10; n++) {
      const session = n === 0
        ? value(await page.evaluate(input => window.shellfox.renameSession(input), { sessionId: initial.sessions[0].id, title: `Second Pi ${n}` }))
        : value(await page.evaluate(input => window.shellfox.createSession(input), { cwd: dir, requestId: randomUUID(), title: `Second Pi ${n}` }));
      expect(session.tabs).toHaveLength(1);
      await page.getByRole('button', { name: `Select session Second Pi ${n}`, exact: true }).click();
      if (n % 2) {
        const first = session.tabs[0];
        value(await page.evaluate(input => window.shellfox.writeTerminal!(input), { tabId: first.id, generation: first.generation!, data: 'pi\r' }));
        await expect.poll(async () => (await snapshot(page)).sessions.find(s => s.id === session.id)!.tabs[0].status, { timeout: 12000 }).toBe('running');
      }
      if (n % 3 === 2) {
        await page.getByRole('button', { name: 'New terminal', exact: true }).click({ button: 'right' });
        await page.getByRole('menuitem', { name: 'Windows PowerShell', exact: true }).click();
      } else await page.getByRole('button', { name: 'New terminal', exact: true }).click();
      await expect.poll(async () => (await snapshot(page)).sessions.find(s => s.id === session.id)!.tabs.length).toBe(2);
      const added = (await snapshot(page)).sessions.find(s => s.id === session.id)!;
      if (n % 2) expect(added.tabs[0].status).toBe('running');
      const tab = added.tabs[1];
      // Wait for shell startup, not Pi startup. This marker cannot match the echoed command.
      value(await page.evaluate(input => window.shellfox.writeTerminal!(input), { tabId: tab.id, generation: tab.generation!, data: `[Console]::WriteLine(('PI_' + 'READY_${n}'))\r` }));
      await expect.poll(async () => app.evaluate((_electron, input) => {
        const replay = (globalThis as any).__shellfoxTest.backend.attach({ tabId: input.id });
        return replay.ok && replay.value.chunks.map((c: any) => c.data).join('').includes(input.marker);
      }, { id: tab.id, marker: `PI_READY_${n}` }), { timeout: 12000 }).toBe(true);
      // The output marker proves the shell executes input, not that its initial
      // process identity has been observed. Do not start Pi while that is unknown.
      await expect.poll(async () => (await snapshot(page)).sessions.find(s => s.id === session.id)!.tabs[1].status, { timeout: 12000 }).toBe('waiting');
      const start = await app.evaluate(() => Date.now());
      let source: { completedAt: number; duration: number; shellPid: number; piPid: number } | null = null;
      let sourceCompletedAt = 0;
      value(await page.evaluate(input => window.shellfox.writeTerminal!(input), { tabId: tab.id, generation: tab.generation!, data: 'pi\r' }));
      try {
        // Windows CIM and the optional running-WSL scan are asynchronous and can
        // exceed 3s under suite load. Wait for independent native evidence of
        // this exact shell's Pi child, not for the classifier's desired status.
        // No forced tracker poll, altered snapshots or synthetic observations.
        await expect.poll(async () => {
          source = await app.evaluate((_electron, input) => {
            for (const trace of (globalThis as any).trackingTrace) {
              if (trace.startedAt < input.startedAt) continue;
              const root = trace.roots.find((r: any) => r.tabId === input.tabId && r.generation === input.generation);
              if (!root) continue;
              for (const s of trace.snapshots) {
                if (s.environment !== 'local' || s.statusOnly || !s.complete) continue;
                const shell = s.processes.find((p: any) => p.pid === root.pid && p.accessible && p.birth);
                const pi = s.processes.find((p: any) => p.parentPid === root.pid && p.accessible && p.birth &&
                  typeof p.executable === 'string' && /[\\/]pi\.exe$/i.test(p.executable));
                if (shell && pi) return { completedAt: trace.completedAt, duration: trace.duration, shellPid: root.pid, piPid: pi.pid };
              }
            }
            return null;
          }, { tabId: tab.id, generation: tab.generation!, startedAt: start });
          if (source) sourceCompletedAt = source.completedAt;
          return source !== null;
        }, { timeout: 15000, intervals: [100, 200, 300] }).toBe(true);
        const remaining = Math.max(1, 3000 - (await app.evaluate(() => Date.now()) - sourceCompletedAt));
        await expect.poll(async () => (await snapshot(page)).sessions.find(s => s.id === session.id)!.tabs[1].status, { timeout: remaining, intervals: [100, 200, 300] }).toBe('running');
        expect(await app.evaluate(() => Date.now()) - sourceCompletedAt, 'Classification must follow complete native evidence within 3s').toBeLessThan(3000);
        expect((await snapshot(page)).sessions.find(s => s.id === session.id)!.tabs[1].agents).toBeGreaterThan(0);
        await expect(page.locator(`#terminal-tab-${tab.id} .status-dot`)).toHaveClass(/dot-(running|busy)/);
        await expect(page.locator(`#terminal-tab-${tab.id} .status-dot`)).toHaveAttribute('aria-label', /Agent (working|idle)/);
      } finally {
        const finishedAt = Date.now();
        evidence.push({ n, elapsedMs: finishedAt - start, source, detectionAfterSnapshotMs: source ? finishedAt - (source as { completedAt: number }).completedAt : null,
          session: (await snapshot(page)).sessions.find(s => s.id === session.id), roots: await app.evaluate(() => (globalThis as any).__shellfoxTest.backend.live()) });
      }
      for (const t of added.tabs) value(await page.evaluate(input => window.shellfox.closeTab!(input), { tabId: t.id, generation: t.generation! }));
    }
  } finally {
    await writeFile(path.join(dir, 'evidence.json'), JSON.stringify({ evidence, trace: await app.evaluate(() => (globalThis as any).trackingTrace) }, null, 2));
    console.log('second Pi evidence', path.join(dir, 'evidence.json'));
    await page.evaluate(async () => { const result = await window.shellfox.getSnapshot(); if (result.ok) for (const s of result.value.sessions) for (const t of s.tabs) if (t.terminalKind === 'embedded' && t.lifecycle !== 'closed') await window.shellfox.closeTab!({ tabId: t.id, generation: t.generation! }); }).catch(() => {});
    await app.close();
  }
});
