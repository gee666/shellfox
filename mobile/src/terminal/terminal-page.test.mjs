import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { readFile } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import { test } from 'node:test';
import { chromium, expect } from '@playwright/test';

const root = fileURLToPath(new URL('../../../', import.meta.url));
const generator = fileURLToPath(new URL('../../../scripts/mobile-terminal.mjs', import.meta.url));
// This suite deliberately uses a real browser. xterm rendering and IME do not work in jsdom.
test('offline bundled xterm bridge in Chromium', { timeout: 90000 }, async t => {
  execFileSync(process.execPath, [generator], { cwd: root, timeout: 30000 });
  const module = await readFile(new URL('./terminal-html.ts', import.meta.url), 'utf8');
  const html = JSON.parse(module.split('export const TERMINAL_HTML = ')[1].trim().slice(0, -1));
  const browser = await chromium.launch({ headless: true });
  t.after(() => browser.close());
  const page = await browser.newPage({ viewport: { width: 800, height: 600 } });
  const messages = [], errors = [], requests = [];
  page.on('pageerror', error => errors.push(error.message));
  page.on('request', request => requests.push(request.url()));
  await page.route('**/*', route => route.abort());
  await page.exposeFunction('__terminalMessage', json => messages.push(JSON.parse(json)));
  await page.evaluate(() => { window.ReactNativeWebView = { postMessage: data => window.__terminalMessage(data) }; });
  await page.setContent(html, { waitUntil: 'load' });
  await expect.poll(() => messages.some(m => m.type === 'ready')).toBe(true);
  const viewId = messages.find(m => m.type === 'ready').viewId;
  let epoch = 'browser-1', ticket = 0;
  const send = async message => page.evaluate(data => window.dispatchEvent(new MessageEvent('message', { data: JSON.stringify(data) })), { viewId, epoch, ...message });
  const output = async (data, historical = false, fit = false) => {
    const current = ++ticket;
    await send({ type: 'output', data, historical, fit, ticket: current });
    await expect.poll(() => messages.some(m => m.type === 'rendered' && m.epoch === epoch && m.ticket === current)).toBe(true);
    return current;
  };
  await send({ type: 'reset', cols: 80, rows: 24, fontSize: 14 });

  await t.test('replay restores ANSI grid before paint receipt, suppresses device replies, then fits', async () => {
    await output('\x1b[2J\x1b[H\x1b[32mREPLAY_SCREEN\x1b[0m\x1b[2;8Hcolumn-eight\x1b[6n', true, true);
    await expect(page.locator('.xterm-rows')).toContainText('REPLAY_SCREEN');
    await expect(page.locator('.xterm-rows')).toContainText('column-eight');
    assert.equal(messages.filter(m => m.type === 'input').length, 0);
    await expect.poll(() => messages.some(m => m.type === 'resize')).toBe(true);
    assert.ok(messages.findIndex(m => m.type === 'rendered') < messages.findIndex(m => m.type === 'resize'));
  });
  await t.test('keyboard, Ctrl shortcuts, arrows, and Unicode IME input use xterm', async () => {
    await send({ type: 'focus', focused: true });
    await page.keyboard.type('hello'); await page.keyboard.press('Enter');
    await page.keyboard.press('Control+c'); await page.keyboard.press('Control+d');
    await page.keyboard.press('ArrowUp'); await page.keyboard.insertText('日本語');
    await expect.poll(() => messages.filter(m => m.type === 'input').map(m => m.data).join('')).toContain('日本語');
    const input = messages.filter(m => m.type === 'input').map(m => m.data).join('');
    for (const text of ['hello', '\r', '\x03', '\x04', '\x1b[A', '日本語']) assert.ok(input.includes(text), JSON.stringify({ input, text }));
  });
  const noModifiers = { ctrl: false, alt: false, shift: false, meta: false, capsLock: false };
  const key = async (key, modifiers = {}) => send({ type: 'key', event: { key, modifiers: { ...noModifiers, ...modifiers } } });
  const receivedInput = () => messages.filter(m => m.type === 'input').map(m => m.data);
  await t.test('accessory keys use xterm encoding and do not focus the hidden IME', async () => {
    await send({ type: 'focus', focused: false });
    const before = receivedInput().length;
    for (const name of ['Escape', 'Tab', 'Enter', 'Backspace', 'ArrowUp', 'Home', 'End', 'PageUp', 'PageDown', 'Insert', 'Delete',
      'F1', 'F2', 'F3', 'F4', 'F5', 'F6', 'F7', 'F8', 'F9', 'F10', 'F11', 'F12']) await key(name);
    const expected = ['\x1b', '\t', '\r', '\x7f', '\x1b[A', '\x1b[H', '\x1b[F', '\x1b[5~', '\x1b[6~', '\x1b[2~', '\x1b[3~',
      '\x1bOP', '\x1bOQ', '\x1bOR', '\x1bOS', '\x1b[15~', '\x1b[17~', '\x1b[18~', '\x1b[19~', '\x1b[20~', '\x1b[21~', '\x1b[23~', '\x1b[24~'];
    await expect.poll(() => receivedInput().slice(before)).toEqual(expected);
    await expect(page.locator('.xterm-helper-textarea')).not.toBeFocused();
    await output('\x1b[?1h');
    const application = receivedInput().length;
    await key('ArrowUp'); await key('Home'); await key('End');
    await expect.poll(() => receivedInput().slice(application)).toEqual(['\x1bOA', '\x1bOH', '\x1bOF']);
    await output('\x1b[?1l');
  });
  await t.test('modifier combinations, sticky IME modifiers, and fresh Ctrl+C events', async () => {
    const before = receivedInput().length;
    await key('Tab', { shift: true });
    await key('ArrowUp', { ctrl: true });
    await key('ArrowRight', { alt: true, shift: true });
    await key('Delete', { ctrl: true, alt: true });
    await key('F12', { shift: true });
    await key('c', { alt: true });
    await key('c', { shift: true });
    await key('c', { capsLock: true });
    await key('c', { shift: true, capsLock: true });
    // Each tap must create a fresh interrupt, not reuse or debounce a DOM event.
    for (let i = 0; i < 4; i++) { await key('c', { ctrl: true }); await key('Escape'); }
    await expect.poll(() => receivedInput().slice(before)).toEqual([
      '\x1b[Z', '\x1b[1;5A', '\x1b[1;4C', '\x1b[3;7~', '\x1b[24;2~', '\x1bc', 'C', 'C', 'c',
      '\x03', '\x1b', '\x03', '\x1b', '\x03', '\x1b', '\x03', '\x1b',
    ]);
    await send({ type: 'modifiers', modifiers: { ...noModifiers, ctrl: true } });
    await send({ type: 'focus', focused: true });
    const sticky = receivedInput().length;
    await page.keyboard.press('c'); await page.keyboard.press('c');
    await page.keyboard.press('ArrowLeft');
    await page.keyboard.insertText('c');
    await page.keyboard.insertText('cc'); // Gboard can commit several ASCII characters in one input event.
    await expect.poll(() => receivedInput().slice(sticky)).toEqual(['\x03', '\x03', '\x1b[1;5D', '\x03', '\x03', '\x03']);
    const replay = receivedInput().length;
    await output('\x1b[6n\x1b[c', true);
    assert.equal(receivedInput().length, replay, 'historical replay must not send terminal query replies or inputs');
    await send({ type: 'paste', data: 'unchanged paste' });
    await expect.poll(() => receivedInput().at(-1)).toBe('unchanged paste');
    await send({ type: 'modifiers', modifiers: noModifiers });
    const plain = receivedInput().length;
    await page.keyboard.type('after');
    await expect.poll(() => receivedInput().slice(plain).join('')).toBe('after');
  });
  await t.test('Super follows xterm semantics without inventing global shortcuts', async () => {
    const before = receivedInput().length;
    await key('c', { meta: true }); // No legacy terminal encoding for Super+C.
    await key('F1', { meta: true }); // xterm does encode modified function keys.
    await expect.poll(() => receivedInput().slice(before)).toEqual(['\x1b[1;9P']);
  });
  await t.test('theme changes repaint the existing terminal without resetting its contents', async () => {
    await send({ type: 'theme', accentColor: '#123456', backgroundColor: '#ffffff' });
    assert.equal(await page.evaluate(() => getComputedStyle(document.body).backgroundColor), 'rgb(255, 255, 255)');
    await expect(page.locator('.xterm-rows')).toContainText('REPLAY_SCREEN');
    await send({ type: 'theme', accentColor: '#ec4899', backgroundColor: '#111016' });
    assert.equal(await page.evaluate(() => getComputedStyle(document.body).backgroundColor), 'rgb(17, 16, 22)');
  });
  await t.test('native paste honors bracketed paste mode', async () => {
    await output('\x1b[?2004h');
    await send({ type: 'paste', data: 'pasted\ntext' });
    await expect.poll(() => messages.filter(m => m.type === 'input').map(m => m.data).join('')).toContain('\x1b[200~pasted\rtext\x1b[201~');
  });
  await t.test('resize and font changes emit bounded terminal dimensions', async () => {
    const before = messages.filter(m => m.type === 'resize').at(-1);
    await page.setViewportSize({ width: 390, height: 450 });
    await expect.poll(() => messages.filter(m => m.type === 'resize').at(-1)?.cols).toBeLessThan(before.cols);
    const smaller = messages.filter(m => m.type === 'resize').at(-1);
    await send({ type: 'font', fontSize: 22 });
    await expect.poll(() => messages.filter(m => m.type === 'resize').at(-1)?.cols).toBeLessThan(smaller.cols);
    for (const m of messages.filter(m => m.type === 'resize')) assert.ok(m.cols >= 2 && m.cols <= 500 && m.rows >= 2 && m.rows <= 500);
  });
  await t.test('output stays literal and cannot load resources or navigate', async () => {
    await output('\r\n</script><img src=https://evil.invalid/x>\r\n\x1b]8;;https://evil.invalid/\x07link-label\x1b]8;;\x07');
    await expect(page.locator('.xterm-rows')).toContainText('</script>');
    assert.equal(await page.locator('img,iframe,script[src],link[href]').count(), 0);
    assert.deepEqual(requests, []);
    await page.evaluate(async () => { try { await fetch('https://evil.invalid/blocked'); } catch { /* CSP must reject before the network. */ } });
    assert.deepEqual(requests, []);
    const link = page.locator('.xterm-rows a', { hasText: 'link-label' });
    if (await link.count()) await link.click();
    assert.equal(page.url(), 'about:blank');
  });
  await t.test('touch selection emits text for the native copy dialog', async () => {
    epoch = 'selection';
    await send({ type: 'reset', cols: 80, rows: 24, fontSize: 14 });
    await output('SELECT_COPY', true, true);
    await send({ type: 'select', enabled: true });
    const cols = messages.filter(m => m.type === 'resize' && m.epoch === epoch).at(-1)?.cols ?? 80;
    await page.evaluate(columns => {
      const screen = document.querySelector('.xterm-screen'), rect = screen.getBoundingClientRect();
      const cellWidth = rect.width / columns;
      for (const [type, col] of [['touchstart', 0], ['touchmove', 10], ['touchend', 10]]) {
        const touch = new Touch({ identifier: 1, target: screen, clientX: rect.left + cellWidth * (col + 0.5), clientY: rect.top + 4 });
        screen.dispatchEvent(new TouchEvent(type, { bubbles: true, cancelable: true, touches: type === 'touchend' ? [] : [touch], changedTouches: [touch] }));
      }
    }, cols);
    await expect.poll(() => messages.filter(m => m.type === 'selection' && m.epoch === epoch).at(-1)?.text).toBe('SELECT_COPY');
  });
  await t.test('Android document messages that bubble are processed once', async () => {
    const current = ++ticket;
    await page.evaluate(data => document.dispatchEvent(new MessageEvent('message', { bubbles: true, data: JSON.stringify(data) })),
      { type: 'output', viewId, epoch, ticket: current, data: '\r\nONCE', historical: false, fit: false });
    await expect.poll(() => messages.filter(m => m.type === 'rendered' && m.ticket === current).length).toBe(1);
    await expect(page.locator('.xterm-rows')).toContainText('ONCE');
  });
  await t.test('generation reset retires old callbacks and rejects stale messages', async () => {
    epoch = 'browser-2';
    await send({ type: 'reset', cols: 80, rows: 24, fontSize: 14 });
    await send({ type: 'output', epoch: 'browser-1', ticket: 900, data: 'STALE', historical: false, fit: false });
    await output('NEW_GENERATION', true, true);
    await expect(page.locator('.xterm-rows')).toContainText('NEW_GENERATION');
    await expect(page.locator('.xterm-rows')).not.toContainText('STALE');
    assert.ok(!messages.some(m => m.type === 'rendered' && m.ticket === 900));
  });
  await t.test('empty replay renders and a reset preserves keyboard focus', async () => {
    await send({ type: 'focus', focused: true });
    await expect(page.locator('.xterm-helper-textarea')).toBeFocused();
    epoch = 'empty';
    await send({ type: 'reset', cols: 80, rows: 24, fontSize: 14 });
    await output('', true, true);
    await expect(page.locator('.xterm-helper-textarea')).toBeFocused();
  });
  assert.deepEqual(errors, []);
});
