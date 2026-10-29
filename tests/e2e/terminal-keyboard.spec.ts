import { test, expect, type ElectronApplication } from '@playwright/test';
import { randomUUID } from 'node:crypto';
import { readFile, writeFile } from 'node:fs/promises';
import path from 'node:path';
import type { PtyBackend } from '../../src/main/terminal/backend';
import { launch, root, scratch, seedTerminal, value } from '../fixtures/electron';

type TestGlobal = typeof globalThis & {
  __shellfoxTest: { backend: PtyBackend };
  __keyboardInput: string[];
};

async function recordInput(app: ElectronApplication, tabId: string) {
  await app.evaluate((_electron, id) => {
    const state = globalThis as TestGlobal;
    const backend = state.__shellfoxTest.backend, write = backend.write.bind(backend);
    state.__keyboardInput = [];
    backend.write = input => {
      const result = write(input);
      if (result.ok && input.tabId === id) state.__keyboardInput.push(input.data);
      return result;
    };
  }, tabId);
}

test('stock xterm sends lowercase, Shift letters, intentional repeats, spaces and Unicode once', async () => {
  const dir = await scratch('terminal-keyboard');
  const { app, page } = await launch(path.join(dir, 'data'), 'real');
  let identity: { tabId: string; generation: string } | undefined;
  try {
    const session = value(await page.evaluate(input => window.shellfox.createSession(input), {
      cwd: dir, requestId: randomUUID(), title: 'Keyboard regression',
    }));
    const tab = session.tabs[0]!;
    if (!tab.generation) throw new Error('Missing terminal generation');
    identity = { tabId: tab.id, generation: tab.generation };
    await page.getByRole('button', { name: 'Select session Keyboard regression', exact: true }).click();
    const textarea = page.locator('.xterm-helper-textarea');
    await expect(textarea).toBeEditable();
    await textarea.focus();
    await recordInput(app, tab.id);
    await page.keyboard.type('abcdefghijklmnopqrstuvwxyz');
    for (const letter of 'ABCDEFGHIJKLMNOPQRSTUVWXYZ') await page.keyboard.press(`Shift+${letter}`);
    await page.keyboard.type('AAZZaa!!  password  ');
    await page.keyboard.press('Space');
    await page.keyboard.press('Shift+Space');
    await page.keyboard.press('Shift+!');
    await page.keyboard.insertText('日本語🙂');
    await page.keyboard.press('Enter');
    await expect.poll(() => app.evaluate(() => (globalThis as TestGlobal).__keyboardInput.join('')))
      .toBe('abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZAAZZaa!!  password    !日本語🙂\r');
  } finally {
    if (identity) await page.evaluate(input => window.shellfox.closeTab!(input), identity).catch(() => {});
    await app.close();
  }
});

test('a real no-echo password reader receives exact letters, repeated spaces and Unicode bytes', async ({}, testInfo) => {
  const dir = await scratch('terminal-password');
  const script = path.join(dir, 'password-reader.cjs'), record = path.join(dir, 'received.bin');
  // This reader never echoes input or receives any user credentials.
  await writeFile(script, `const fs = require('node:fs');
const chunks = [];
process.stdin.setRawMode(true);
process.stdin.resume();
process.stdin.on('data', bytes => {
  const end = bytes.indexOf(13);
  chunks.push(end < 0 ? bytes : bytes.subarray(0, end));
  if (end < 0) return;
  fs.writeFileSync(process.argv[2], Buffer.concat(chunks));
  process.stdin.setRawMode(false);
  process.stdout.write('\\r\\nPASSWORD_RECORDED\\r\\n');
  process.exit(0);
});
process.stdout.write('\\x1b[2J\\x1b[HPASSWORD_READY: ');
`);
  const { app, page } = await launch(path.join(dir, 'data'), 'real');
  let identity: { tabId: string; generation: string } | undefined;
  try {
    const session = value(await page.evaluate(input => window.shellfox.createSession(input), {
      cwd: dir, requestId: randomUUID(), title: 'No-echo keyboard regression',
    }));
    const tab = session.tabs[0]!;
    if (!tab.generation) throw new Error('Missing terminal generation');
    identity = { tabId: tab.id, generation: tab.generation };
    await page.getByRole('button', { name: 'Select session No-echo keyboard regression', exact: true }).click();
    const powershell = ['pwsh', 'windows-powershell'].includes(tab.profileId ?? '');
    const quote = (text: string) => "'" + text.replaceAll("'", powershell ? "''" : "'\\''") + "'";
    const runtime = process.platform === 'win32' ? path.join(root, 'tmp/build-test/cli/cli-runtime/node.exe') : process.execPath;
    const command = (powershell ? '& ' : '') + [runtime, script, record].map(quote).join(' ') + '\r';
    value(await seedTerminal(app, { ...identity, data: command }));
    const rows = page.locator('.xterm-rows');
    await expect(rows).toContainText('PASSWORD_READY:');
    const prompt = await rows.innerText();
    const textarea = page.locator('.xterm-helper-textarea');
    await expect(textarea).toBeEditable();
    await textarea.focus();
    await recordInput(app, tab.id);
    await page.keyboard.press('Space'); await page.keyboard.press('Space');
    await page.keyboard.press('Shift+A'); await page.keyboard.press('Shift+A');
    await page.keyboard.type('aaZZ!!  password');
    await page.keyboard.press('Space'); await page.keyboard.press('Space');
    await page.keyboard.press('Shift+Space'); await page.keyboard.press('Shift+Space');
    // insertText exercises Chromium text insertion, not a platform IME candidate UI.
    await page.keyboard.insertText('日本語🙂');
    await textarea.evaluate(element => {
      const clipboardData = new DataTransfer();
      clipboardData.setData('text/plain', ' native貼付け🙂 ');
      element.dispatchEvent(new ClipboardEvent('paste', { clipboardData, bubbles: true, cancelable: true }));
    });
    // Stub only the app's clipboard read, leaving the user's host clipboard untouched.
    await app.evaluate(({ clipboard }, text) => { clipboard.readText = async () => text; }, ' shortcut貼付け🙂 ');
    await page.keyboard.press('Control+Shift+V');
    // The clipboard bridge is asynchronous. Wait for its bytes before typing on.
    await expect.poll(() => app.evaluate(() => (globalThis as TestGlobal).__keyboardInput.join('')))
      .toContain(' shortcut貼付け🙂 ');
    await page.keyboard.type('  ');
    const fixture = '  AAaaZZ!!  password    日本語🙂 native貼付け🙂  shortcut貼付け🙂   ';
    await expect(rows).toHaveText(prompt);
    await page.keyboard.press('Enter');
    await expect(rows).toContainText('PASSWORD_RECORDED');
    const received = await readFile(record), expected = Buffer.from(fixture, 'utf8');
    await testInfo.attach('synthetic-password-bytes.json', {
      body: JSON.stringify({ expectedBytes: expected.byteLength, receivedBytes: received.byteLength,
        expectedHex: expected.toString('hex'), receivedHex: received.toString('hex') }),
      contentType: 'application/json',
    });
    expect(received.toString('hex')).toBe(expected.toString('hex'));
    expect(received.byteLength).toBe(expected.byteLength);
  } finally {
    if (identity) await page.evaluate(input => window.shellfox.closeTab!(input), identity).catch(() => {});
    await app.close();
  }
});
