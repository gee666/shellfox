import { test, expect } from '@playwright/test';
import { randomUUID } from 'node:crypto';
import { rm, writeFile } from 'node:fs/promises';
import path from 'node:path';
import type { PtyBackend } from '../../src/main/terminal/backend';
import { launch, root, scratch, seedTerminal, snapshot, value } from '../fixtures/electron';

type TestGlobal = typeof globalThis & {
  __shellfoxTest: { backend: PtyBackend };
  __settlementInput: { data: string; userInitiated?: boolean }[];
};

test('a mouse-reporting TUI stays settled through hover and focus until real input resumes it', async () => {
  const dir = await scratch('terminal-settlement');
  const script = path.join(dir, 'mouse-tui.cjs');
  await writeFile(script, `process.stdin.setRawMode(true);
process.stdin.resume();
process.stdin.on('data', () => {});
process.stdout.write('\\x1b[?1003h\\x1b[?1006h\\x1b[?1004h\\r\\nSETTLEMENT_TUI_READY\\r\\n');
`);
  const { app, page } = await launch(path.join(dir, 'data'), 'real');
  let identity: { tabId: string; generation: string } | undefined;
  try {
    const session = value(await page.evaluate(input => window.shellfox.createSession(input), {
      cwd: dir, requestId: randomUUID(), title: 'Settlement regression',
    }));
    const tab = session.tabs[0]!;
    if (!tab.generation) throw new Error('Missing terminal generation');
    identity = { tabId: tab.id, generation: tab.generation };
    await page.getByRole('button', { name: 'Select session Settlement regression', exact: true }).click();
    const powershell = ['pwsh', 'windows-powershell'].includes(tab.profileId ?? '');
    const quote = (text: string) => "'" + text.replaceAll("'", powershell ? "''" : "'\\''") + "'";
    const runtime = process.platform === 'win32' ? path.join(root, 'tmp/build-test/cli/cli-runtime/node.exe') : process.execPath;
    const command = (powershell ? '& ' : '') + [runtime, script].map(quote).join(' ') + '\r';
    value(await seedTerminal(app, { ...identity, data: command }));
    await expect(page.locator('.xterm-rows')).toContainText('SETTLEMENT_TUI_READY');
    await app.evaluate((_electron, tabId) => {
      const state = globalThis as TestGlobal;
      const backend = state.__shellfoxTest.backend, write = backend.write.bind(backend);
      state.__settlementInput = [];
      backend.write = input => {
        const result = write(input);
        if (result.ok && input.tabId === tabId) state.__settlementInput.push({ data: input.data, userInitiated: input.userInitiated });
        return result;
      };
    }, tab.id);
    const current = async () => (await snapshot(page)).sessions.find(item => item.id === session.id)!;
    for (let attempt = 0; attempt < 3; attempt++) {
      await page.getByRole('button', { name: 'Settle session Settlement regression', exact: true }).click();
      await expect.poll(async () => (await current()).status).toBe('settled');
      const marker = (await current()).settledAt;
      const before = await app.evaluate(() => (globalThis as TestGlobal).__settlementInput.length);
      const rect = await page.locator('.xterm-screen').boundingBox();
      if (!rect) throw new Error('Missing terminal screen');
      await page.mouse.move(rect.x + 30, rect.y + 30);
      await page.mouse.move(rect.x + 60, rect.y + 30);
      await page.locator('.xterm-helper-textarea').focus();
      await expect.poll(() => app.evaluate((_electron, start) => (globalThis as TestGlobal).__settlementInput.slice(start)
        .filter(input => /^\x1b\[<35;/.test(input.data)).length, before)).toBeGreaterThan(0);
      await expect.poll(() => app.evaluate((_electron, start) => (globalThis as TestGlobal).__settlementInput.slice(start)
        .some(input => input.data === '\x1b[I'), before)).toBe(true);
      expect(await current()).toMatchObject({ status: 'settled', settledAt: marker });
      const reports = await app.evaluate((_electron, start) => (globalThis as TestGlobal).__settlementInput.slice(start), before);
      expect(reports.every(input => input.userInitiated === false)).toBe(true);
      await page.keyboard.type('x');
      await expect.poll(async () => (await current()).settledAt).toBeNull();
    }
  } finally {
    if (identity) await page.evaluate(input => window.shellfox.closeTab!(input), identity).catch(() => {});
    await app.close();
    await rm(dir, { recursive: true, force: true });
  }
});
