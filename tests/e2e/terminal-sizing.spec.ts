import { test, expect } from '@playwright/test';
import { randomUUID } from 'node:crypto';
import path from 'node:path';
import type { PtyBackend } from '../../src/main/terminal/backend';
import type { EmbeddedSessionService } from '../../src/main/terminal/service';
import type { TerminalControlView } from '../../src/main/terminal/control';
import { launch, scratch, value } from '../fixtures/electron';

type TestGlobal = typeof globalThis & {
  __shellfoxTest: { backend: PtyBackend; service: EmbeddedSessionService };
  __terminalControlView?: TerminalControlView;
};

test('exclusive control clears the desktop, promotes it on disconnect, and permits explicit Take it', async () => {
  test.setTimeout(90000);
  const dir = await scratch('terminal-control'), { app, page } = await launch(path.join(dir, 'data'), 'real');
  try {
    const session = value(await page.evaluate(input => window.shellfox.createSession(input), { cwd: dir, requestId: randomUUID(), title: 'Control regression' }));
    const tab = session.tabs[0]!;
    if (!tab.generation) throw new Error('Missing terminal generation');
    const identity = { tabId: tab.id, generation: tab.generation };
    await page.getByRole('button', { name: 'Select session Control regression', exact: true }).click();
    await expect(page.getByLabel('Interactive terminal')).toBeVisible();
    const size = () => app.evaluate((_electron, id) => {
      const terminal = (globalThis as TestGlobal).__shellfoxTest.backend.get(id);
      return terminal ? { cols: terminal.cols, rows: terminal.rows } : null;
    }, tab.id);
    await expect.poll(async () => (await size())?.cols ?? 0).toBeGreaterThan(52);
    const desktop = await size();
    const waiting = value(await app.evaluate((_electron, id) => {
      const state = globalThis as TestGlobal;
      state.__terminalControlView = state.__shellfoxTest.service.createTerminalView(() => {});
      return state.__terminalControlView.attach({ tabId: id });
    }, tab.id));
    expect(waiting.control).toBe('blocked');
    expect(Object.keys(waiting).sort()).toEqual(['control', 'controlEpoch', 'generation', 'sessionId', 'tabId']);
    const denied = await app.evaluate((_electron, input) => {
      const view = (globalThis as TestGlobal).__terminalControlView;
      if (!view) throw new Error('Missing second client');
      return view.resize(input);
    }, { ...identity, controlEpoch: waiting.controlEpoch, cols: 52, rows: 24 });
    expect(denied.ok).toBe(false); expect(await size()).toEqual(desktop);
    const other = value(await app.evaluate((_electron, id) => {
      const view = (globalThis as TestGlobal).__terminalControlView;
      if (!view) throw new Error('Missing second client');
      const taken = view.take(id); if (!taken.ok) return taken;
      return view.attach({ tabId: id.tabId });
    }, identity));
    expect(other.control).toBe('owned');
    await expect(page.getByText('Another client is controlling this terminal', { exact: true })).toBeVisible();
    await expect(page.getByLabel('Interactive terminal')).toBeHidden();
    value(await app.evaluate((_electron, input) => {
      const view = (globalThis as TestGlobal).__terminalControlView;
      if (!view) throw new Error('Missing second client');
      return view.resize(input);
    }, { ...identity, controlEpoch: other.controlEpoch, cols: 52, rows: 24 }));
    expect(await size()).toEqual({ cols: 52, rows: 24 });
    await app.evaluate(() => { const state = globalThis as TestGlobal; state.__terminalControlView?.dispose(); state.__terminalControlView = undefined; });
    await expect(page.getByLabel('Interactive terminal')).toBeVisible();
    await expect.poll(size).toEqual(desktop);
    // A new waiting client takes the same shell; the desktop can take it back.
    value(await app.evaluate((_electron, id) => {
      const state = globalThis as TestGlobal, view = state.__shellfoxTest.service.createTerminalView(() => {});
      state.__terminalControlView = view;
      const attached = view.attach({ tabId: id.tabId }); if (!attached.ok) return attached;
      return view.take(id);
    }, identity));
    await expect(page.getByRole('button', { name: 'Take it', exact: true })).toBeVisible();
    await page.getByRole('button', { name: 'Take it', exact: true }).click();
    await expect(page.getByLabel('Interactive terminal')).toBeVisible();
    await expect.poll(size).toEqual(desktop);
    const stillWaiting = value(await app.evaluate((_electron, id) => {
      const view = (globalThis as TestGlobal).__terminalControlView;
      if (!view) throw new Error('Missing second client');
      return view.attach({ tabId: id });
    }, tab.id));
    expect(stillWaiting.control).toBe('blocked');
    value(await page.evaluate(input => window.shellfox.closeTab!(input), identity));
  } finally {
    await app.evaluate(() => { (globalThis as TestGlobal).__terminalControlView?.dispose(); }).catch(() => {});
    await app.close();
  }
});
