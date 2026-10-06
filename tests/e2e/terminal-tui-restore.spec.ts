import { test, expect } from '@playwright/test';
import { randomUUID } from 'node:crypto';
import { writeFile } from 'node:fs/promises';
import path from 'node:path';
import { mkdir } from 'node:fs/promises';
import { launch, scratch, snapshot, value } from '../fixtures/electron';

// A diff-rendering full-screen TUI (like the pi agent): one full paint, then only tiny
// cursor-addressed updates inside DEC 2026 synchronized output. Writes `TUI_DONE` when finished.
const TUI = `
const out = process.stdout, cols = out.columns || 100, rows = out.rows || 30, ESC = '\\x1b';
const at = (r, c) => ESC + '[' + r + ';' + c + 'H', pad = (t, w) => (t + ' '.repeat(w)).slice(0, w);
let first = ESC + '[?25l' + ESC + '[?2026h' + ESC + '[2J';
for (let r = 1; r <= rows; r++) first += at(r, 1) + ESC + '[1m' + pad(r === 1 ? 'TUI header' : r === rows ? 'TUI footer' : 'agent ' + r + ' idle', 30) + ESC + '[0m' + ESC + '[48;2;60;50;40m' + pad('  last action: 00:00:' + r, Math.max(10, cols - 30)) + ESC + '[0m';
out.write(first + ESC + '[?2026l');
let frame = 0;
function step() {
  for (let i = 0; i < 40 && frame < 4000; i++, frame++) {
    const r = 3 + (frame % (rows - 6));
    out.write(ESC + '[?2026h' + at(r, 31) + ESC + '[48;2;60;50;40m' + pad('  last action: 00:' + frame, Math.max(10, cols - 30)) + ESC + '[0m' + at(rows - 2, 1) + ESC + '[2KWorking ' + frame + ESC + '[?2026l');
  }
  if (frame < 4000) setTimeout(step, 4); else { out.write(at(rows - 2, 1) + ESC + '[2KTUI_DONE'); setInterval(() => {}, 1000); }
}
// Hold the flood until the test has left this tab, so it happens entirely while the view is detached.
out.write(at(rows - 2, 1) + ESC + '[2KWorking 0');
const wait = setInterval(() => { if (require('fs').existsSync(process.argv[2])) { clearInterval(wait); step(); } }, 50);
`;
const shots = path.resolve('tmp/e2e-evidence');
const rowsText = (page: import('@playwright/test').Page) => page.locator('.xterm-rows > div').allInnerTexts();

test('a full-screen TUI keeps every row when its tab is left, flooded in the background and shown again', async () => {
  test.skip(process.platform === 'win32', 'The fixture TUI runs in a POSIX shell.');
  test.setTimeout(120000);
  await mkdir(shots, { recursive: true });
  const dir = await scratch('tui-restore'), { app, page } = await launch(path.join(dir, 'data'), 'real');
  try {
    const script = path.join(dir, 'tui.cjs'), go = path.join(dir, 'go'); await writeFile(script, TUI);
    const one = value(await page.evaluate(input => window.shellfox.createSession(input), { cwd: dir, requestId: randomUUID(), title: 'TUI host' }));
    const tab = one.tabs[0]!;
    await page.getByRole('button', { name: 'Select session TUI host', exact: true }).click();
    await expect(page.getByLabel('Interactive terminal')).toBeVisible();
    await expect.poll(async () => (await snapshot(page)).sessions.find(s => s.id === one.id)!.tabs[0]!.status, { timeout: 20000 }).not.toBe('opening');
    value(await page.evaluate(input => window.shellfox.writeTerminal!(input), { tabId: tab.id, generation: tab.generation!, data: `clear; "${process.execPath}" "${script}" "${go}"\r` }));
    await expect.poll(async () => (await rowsText(page)).some(row => /Working \d+/.test(row)), { timeout: 20000 }).toBe(true);

    await page.screenshot({ path: path.join(shots, 'tui-1-running-before-switch.png') });

    // Leave the tab. The TUI keeps going and writes far more than the replay window while hidden.
    value(await page.evaluate(input => window.shellfox.createSession(input), { cwd: dir, requestId: randomUUID(), title: 'Other' }));
    await page.getByRole('button', { name: 'Select session Other', exact: true }).click();
    await page.screenshot({ path: path.join(shots, 'tui-2-other-session.png') });
    await writeFile(go, 'go');
    await expect.poll(() => app.evaluate((_electron, id) => {
      const replay = (globalThis as any).__shellfoxTest.backend.attach({ tabId: id });
      return replay.ok && replay.value.chunks.at(-1)?.data.includes('TUI_DONE') ? replay.value.lastSequence : 0;
    }, tab.id), { timeout: 60000 }).toBeGreaterThan(200);

    await page.getByRole('button', { name: 'Select session TUI host', exact: true }).click();
    await expect.poll(async () => (await rowsText(page)).some(row => row.includes('TUI_DONE')), { timeout: 20000 }).toBe(true);
    await page.waitForTimeout(500);
    await page.screenshot({ path: path.join(shots, 'tui-3-after-switch-back.png') });
    const rows = await rowsText(page);
    // Rows that the TUI painted once and never touched again must still be there.
    expect(rows.filter(row => /agent \d+ idle/.test(row)).length, rows.join('\n')).toBeGreaterThan(rows.length - 10);
    expect(rows.some(row => row.includes('TUI header'))).toBe(true);
    await expect(page.getByText('Earlier output unavailable')).toHaveCount(0);
  } finally {
    await page.evaluate(async () => { const result = await window.shellfox.getSnapshot(); if (result.ok) for (const s of result.value.sessions) for (const t of s.tabs) if (t.terminalKind === 'embedded' && t.lifecycle !== 'closed') await window.shellfox.closeTab!({ tabId: t.id, generation: t.generation! }); }).catch(() => {});
    await app.close();
  }
});
