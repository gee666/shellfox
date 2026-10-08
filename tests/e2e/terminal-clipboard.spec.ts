import { test, expect } from '@playwright/test';
import { randomUUID } from 'node:crypto';
import path from 'node:path';
import { launch, scratch, snapshot, value } from '../fixtures/electron';

test('terminal clipboard shortcuts use the host clipboard for local and WSL shells', async () => {
  test.setTimeout(180000);
  const dir = await scratch('clipboard'), { app, page } = await launch(path.join(dir, 'data'), 'real');
  try {
    // Exercise native IPC without reading or overwriting the user's clipboard.
    await app.evaluate(({ clipboard }) => {
      const state = (globalThis as any).__clipboardTest = { text: '', copies: 0, reads: 0 };
      clipboard.writeText = async text => { state.text = text; state.copies++; };
      clipboard.readText = async () => { state.reads++; return state.text; };
    });
    const initial = await snapshot(page);
    const profiles = value(await page.evaluate(() => window.shellfox.getTerminalProfiles!())).profiles.filter(p => p.available);
    expect(profiles.length).toBeGreaterThan(0);
    for (const profile of profiles) {
      console.log('Clipboard profile:', profile.id);
      await app.evaluate(() => { (globalThis as any).__clipboardTest.text = ''; });
      value(await page.evaluate(settings => window.shellfox.saveSettings(settings), { ...initial.settings, shellExecutable: null, terminalProfileId: profile.id }));
      const session = value(await page.evaluate(input => window.shellfox.createSession(input), { cwd: dir, requestId: randomUUID(), title: profile.id }));
      let tab = session.tabs[0]!;
      if (tab.profileId !== profile.id) {
        const original = tab;
        tab = value(await page.evaluate(input => window.shellfox.addTab(input), { sessionId: session.id, profileId: profile.id })).tabs.at(-1)!;
        value(await page.evaluate(input => window.shellfox.closeTab!(input), { tabId: original.id, generation: original.generation! }));
      }
      await page.getByRole('button', { name: `Select session ${profile.id}`, exact: true }).click();
      await page.getByRole('tab').last().click();
      const textarea = page.locator('.xterm-helper-textarea');
      await expect(textarea).toBeEditable();
      const powershell = ['pwsh', 'windows-powershell'].includes(profile.id);
      const command = powershell
        ? "[Console]::Write(([char]27 + '[2J' + [char]27 + '[HCLIPBOARD_COPY_OK' + [char]27 + '[?1003h' + [char]27 + '[?1006h')); [Console]::WriteLine()\r"
        : "printf '\\033[2J\\033[HCLIPBOARD_COPY_OK\\033[?1003h\\033[?1006h\\n'\r";
      value(await page.evaluate(input => window.shellfox.writeTerminal!(input), { tabId: tab.id, generation: tab.generation!, data: command }));
      await expect.poll(() => app.evaluate((_electron, tabId) => {
        const replay = (globalThis as any).__shellfoxTest.backend.attach({ tabId });
        return replay.ok && replay.value.chunks.map((c: any) => c.data).join('').includes('\x1b[HCLIPBOARD_COPY_OK');
      }, tab.id)).toBe(true);
      await page.waitForTimeout(500);
      const screen = await page.locator('.xterm-screen').boundingBox();
      expect(screen).not.toBeNull();
      await page.keyboard.down('Shift');
      await page.mouse.dblclick(screen!.x + 10, screen!.y + 8);
      await page.keyboard.up('Shift');
      // Any-motion SGR reports used to clear this selection on hover.
      await page.mouse.move(screen!.x + 100, screen!.y + 8);
      await page.keyboard.press('Control+Shift+C');
      await expect.poll(() => app.evaluate(() => (globalThis as any).__clipboardTest.text)).toBe('CLIPBOARD_COPY_OK');
      // Split the marker in the command so only executed output can satisfy the assertion.
      const paste = powershell ? "Write-Output ('CLIPBOARD_' + 'PASTE_OK')" : "printf 'CLIPBOARD_%s\\n' PASTE_OK";
      await app.evaluate((_electron, text) => { (globalThis as any).__clipboardTest.text = text; (globalThis as any).__clipboardTest.reads = 0; }, paste);
      await page.keyboard.press('Control+Shift+V');
      await expect.poll(() => app.evaluate(() => (globalThis as any).__clipboardTest.reads)).toBe(1);
      await expect.poll(() => app.evaluate((_electron, tabId) => {
        const replay = (globalThis as any).__shellfoxTest.backend.attach({ tabId });
        return replay.ok && replay.value.chunks.map((c: any) => c.data).join('').includes('PASTE_OK');
      }, tab.id)).toBe(true);
      await page.keyboard.press('Enter');
      await expect.poll(() => app.evaluate((_electron, tabId) => {
        const replay = (globalThis as any).__shellfoxTest.backend.attach({ tabId });
        return replay.ok ? replay.value.chunks.map((c: any) => c.data).join('') : '';
      }, tab.id)).toContain('CLIPBOARD_PASTE_OK');
      value(await page.evaluate(input => window.shellfox.closeTab!(input), { tabId: tab.id, generation: tab.generation! }));
    }
  } finally {
    await page.evaluate(async () => {
      const result = await window.shellfox.getSnapshot();
      if (result.ok) for (const s of result.value.sessions) for (const t of s.tabs) if (t.terminalKind === 'embedded' && t.lifecycle !== 'closed') await window.shellfox.closeTab!({ tabId: t.id, generation: t.generation! });
    }).catch(() => {});
    await app.close();
  }
});
