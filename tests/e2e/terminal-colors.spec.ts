import { test, expect } from '@playwright/test';
import { randomUUID } from 'node:crypto';
import path from 'node:path';
import { readFile } from 'node:fs/promises';
import { launch, scratch, snapshot, value } from '../fixtures/electron';

test('real local and WSL PTYs advertise truecolor and preserve RGB cells through xterm 6', async () => {
  test.setTimeout(120000);
  const dir = await scratch('truecolor'), { app, page } = await launch(path.join(dir, 'data'), 'real');
  try {
    // DevTools evaluates the unmodified xterm bundle without weakening the app CSP.
    await page.evaluate(await readFile(path.resolve('node_modules/@xterm/xterm/lib/xterm.js'), 'utf8'));
    const initial = await snapshot(page);
    const profiles = value(await page.evaluate(() => window.shellfox.getTerminalProfiles!())).profiles.filter(p => p.available);
    expect(profiles.length).toBeGreaterThan(0);
    for (const profile of profiles) {
      value(await page.evaluate(settings => window.shellfox.saveSettings(settings), { ...initial.settings, shellExecutable: null, terminalProfileId: profile.id }));
      const session = value(await page.evaluate(input => window.shellfox.createSession(input), { cwd: dir, requestId: randomUUID(), title: profile.label }));
      const tab = session.tabs[0];
      const powershell = ['pwsh', 'windows-powershell'].includes(profile.id);
      const command = powershell
        ? "[Console]::WriteLine('COLOR_ENV=' + $env:TERM + ',' + $env:COLORTERM); [Console]::WriteLine(([char]27 + '[38;2;255;100;0mRGB_TEST' + [char]27 + '[0m')); $g=''; 0..63 | ForEach-Object { $g += [char]27 + '[38;2;' + ($_ * 4) + ';' + (255 - $_ * 4) + ';123m#' }; [Console]::WriteLine($g + [char]27 + '[0m'); [Console]::WriteLine('COLOR_DONE')\r"
        : "printf 'COLOR_ENV=%s,%s\\n' \"$TERM\" \"$COLORTERM\"; printf '\\033[38;2;255;100;0mRGB_TEST\\033[0m\\n'; awk 'BEGIN { for (i=0;i<64;i++) printf \"\\033[38;2;%d;%d;123m#\",i*4,255-i*4; printf \"\\033[0m\\n\" }'; echo COLOR_DONE\r";
      value(await page.evaluate(input => window.shellfox.writeTerminal!(input), { tabId: tab.id, generation: tab.generation!, data: command }));
      await expect.poll(async () => {
        const replay: any = value(await app.evaluate((_electron, tabId) => (globalThis as any).__shellfoxTest.backend.attach({ tabId }), tab.id));
        return page.evaluate(async (chunks: Array<{ data: string }>) => {
          const terminal = new (window as any).Terminal({ allowProposedApi: true, cols: 500, rows: 100, scrollback: 3000 });
          await new Promise<void>(resolve => terminal.write(chunks.map(c => c.data).join(''), resolve));
          const lines = Array.from({ length: terminal.buffer.active.length }, (_, y) => terminal.buffer.active.getLine(y).translateToString(true));
          terminal.dispose();
          return lines.find(line => line.trim().startsWith('COLOR_ENV='))?.trim() ?? '';
        }, replay.chunks);
      }, { timeout: 15000 }).toBe('COLOR_ENV=xterm-256color,truecolor');
      const replay: any = value(await app.evaluate((_electron, tabId) => (globalThis as any).__shellfoxTest.backend.attach({ tabId }), tab.id));
      const colors = await page.evaluate(async (chunks: Array<{ data: string }>) => {
        const terminal = new (window as any).Terminal({ allowProposedApi: true, cols: 500, rows: 100, scrollback: 3000 });
        await new Promise<void>(resolve => terminal.write(chunks.map(c => c.data).join(''), resolve));
        let test: any = null, gradient: number[] = [];
        for (let y = 0; y < terminal.buffer.active.length; y++) {
          const line = terminal.buffer.active.getLine(y), text = line.translateToString(true).trim();
          if (text === 'RGB_TEST') { const cell = line.getCell(line.translateToString(true).indexOf('RGB_TEST')); test = { rgb: !!cell.isFgRGB(), color: cell.getFgColor() }; }
          if (text === '#'.repeat(64)) for (let x = 0; x < 64; x++) { const cell = line.getCell(x); if (cell.isFgRGB()) gradient.push(cell.getFgColor()); }
        }
        terminal.dispose(); return { test, gradient };
      }, replay.chunks);
      console.log(profile.id, colors.test, 'RGB gradient cells', colors.gradient.length);
      expect(colors.test).toEqual({ rgb: true, color: 0xff6400 });
      expect(colors.gradient).toEqual(Array.from({ length: 64 }, (_, i) => (i * 4 << 16) | ((255 - i * 4) << 8) | 123));
      value(await page.evaluate(input => window.shellfox.setSessionEnv(input), { sessionId: session.id, env: [{ name: 'TERM', value: 'vt100' }, { name: 'COLORTERM', value: '24bit' }] }));
      const overridden = value(await page.evaluate(sessionId => window.shellfox.addTab({ sessionId }), session.id)).tabs[1];
      const printOverride = powershell
        ? "[Console]::WriteLine(('OVERRIDE_' + 'ENV=') + $env:TERM + ',' + $env:COLORTERM)\r"
        : "printf 'OVERRIDE_%s=%s,%s\\n' ENV \"$TERM\" \"$COLORTERM\"\r";
      value(await page.evaluate(input => window.shellfox.writeTerminal!(input), { tabId: overridden.id, generation: overridden.generation!, data: printOverride }));
      await expect.poll(async () => app.evaluate((_electron, tabId) => {
        const replay = (globalThis as any).__shellfoxTest.backend.attach({ tabId });
        return replay.ok && replay.value.chunks.map((c: any) => c.data).join('').includes('OVERRIDE_ENV=vt100,24bit');
      }, overridden.id), { timeout: 15000 }).toBe(true);
      for (const t of [tab, overridden]) value(await page.evaluate(input => window.shellfox.closeTab!(input), { tabId: t.id, generation: t.generation! }));
    }
  } finally {
    await page.evaluate(async () => { const result = await window.shellfox.getSnapshot(); if (result.ok) for (const s of result.value.sessions) for (const t of s.tabs) if (t.terminalKind === 'embedded' && t.lifecycle !== 'closed') await window.shellfox.closeTab!({ tabId: t.id, generation: t.generation! }); }).catch(() => {});
    await app.close();
  }
});
