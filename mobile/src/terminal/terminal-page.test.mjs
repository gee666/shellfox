import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { readFile } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import { test } from 'node:test';
import { chromium, expect } from '@playwright/test';
import { terminalInputCases } from './terminal-input-cases.mjs';

const root = fileURLToPath(new URL('../../../', import.meta.url));
const generator = fileURLToPath(new URL('../../../scripts/mobile-terminal.mjs', import.meta.url));
// xterm rendering and IME need a real browser. Subtests share one terminal's modes,
// history and attachment epochs, so the lifecycle scenarios stay in one fixture.
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
  await t.test('live query replies are passive while keys, paste and mouse remain user input during output', async () => {
    await output('\x1b[?1000h\x1b[?1006h');
    const before = messages.length;
    const current = ++ticket;
    await page.evaluate(({ viewId, epoch, ticket }) => {
      const send = message => window.dispatchEvent(new MessageEvent('message', { data: JSON.stringify({ viewId, epoch, ...message }) }));
      send({ type: 'output', data: '\x1b[6n', historical: false, fit: false, ticket });
      send({ type: 'paste', data: 'during-output' });
      send({ type: 'key', event: { key: 'Enter', modifiers: { ctrl: false, alt: false, shift: false, meta: false, capsLock: false } } });
      const screen = document.querySelector('.xterm-screen'), rect = screen.getBoundingClientRect();
      screen.dispatchEvent(new WheelEvent('wheel', { bubbles: true, cancelable: true, clientX: rect.x + 5, clientY: rect.y + 5, deltaY: 1, deltaMode: WheelEvent.DOM_DELTA_LINE }));
    }, { viewId, epoch, ticket: current });
    await expect.poll(() => messages.some(m => m.type === 'rendered' && m.ticket === current)).toBe(true);
    const input = messages.slice(before).filter(m => m.type === 'input');
    assert.ok(input.some(m => /^\x1b\[\d+;\d+R$/.test(m.data) && m.userInitiated === false), 'query replies must not resume settled sessions');
    for (const data of ['during-output', '\r']) assert.ok(input.some(m => m.data === data && m.userInitiated !== false));
    assert.ok(input.some(m => /^\x1b\[<65;/.test(m.data) && m.userInitiated !== false), 'mouse reports remain user input during output');
    await output('\x1b[?1000l\x1b[?1006l');
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
  await t.test('Ctrl+J IME input never replays composing text as literal letters', async () => {
    await send({ type: 'modifiers', modifiers: { ...noModifiers, ctrl: true } });
    const before = receivedInput().length;
    await page.evaluate(async () => {
      const textarea = document.querySelector('.xterm-helper-textarea');
      textarea.value = '';
      textarea.dispatchEvent(new CompositionEvent('compositionstart', { bubbles: true }));
      textarea.value = 'j';
      textarea.dispatchEvent(new CompositionEvent('compositionupdate', { bubbles: true, data: 'j' }));
      await new Promise(resolve => setTimeout(resolve, 0));
      textarea.dispatchEvent(new InputEvent('input', { bubbles: true, inputType: 'insertText', data: 'j' }));
      textarea.dispatchEvent(new CompositionEvent('compositionend', { bubbles: true, data: 'j' }));
      await new Promise(resolve => setTimeout(resolve, 20));
    });
    assert.deepEqual(receivedInput().slice(before), ['\n']);
    await send({ type: 'modifiers', modifiers: noModifiers });
  });
  const compositionSequence = async steps => page.evaluate(async ({ steps, viewId, epoch, noModifiers }) => {
    const textarea = document.querySelector('.xterm-helper-textarea');
    textarea.value = '';
    const send = message => window.dispatchEvent(new MessageEvent('message', { data: JSON.stringify({ viewId, epoch, ...message }) }));
    for (const [type, value, data] of steps) {
      if (type === 'start') textarea.dispatchEvent(new CompositionEvent('compositionstart', { bubbles: true }));
      if (type === 'update') {
        textarea.value = value;
        textarea.dispatchEvent(new CompositionEvent('compositionupdate', { bubbles: true, data: data ?? value }));
      }
      if (type === 'input') textarea.dispatchEvent(new InputEvent('input', { bubbles: true, inputType: data ?? 'insertText', data: value }));
      if (type === 'end') textarea.dispatchEvent(new CompositionEvent('compositionend', { bubbles: true, data: value ?? '' }));
      if (type === 'wait') await new Promise(resolve => setTimeout(resolve, 0));
      if (type === 'modifiers') send({ type, modifiers: { ...noModifiers, ...value } });
      if (type === 'key') send({ type, event: { key: value, modifiers: noModifiers } });
      if (type === 'paste') send({ type, data: value });
      if (type === 'reset') send({ type, epoch: value, cols: 80, rows: 24, fontSize: 14 });
    }
    await new Promise(resolve => setTimeout(resolve, 30));
  }, { steps, viewId, epoch, noModifiers });
  await t.test('compositions retain their modifiers and order accessory keys after committed text', async () => {
    const before = receivedInput().length;
    await compositionSequence([
      ['modifiers', { ctrl: true }], ['start'], ['update', 'j'], ['wait'], ['input', 'j'],
      ['key', 'Escape'], ['paste', 'unchanged'], ['modifiers', {}], ['end'],
    ]);
    assert.deepEqual(receivedInput().slice(before), ['\n', '\x1b', 'unchanged']);
    const plain = receivedInput().length;
    await compositionSequence([
      ['start'], ['update', 'j'], ['wait'], ['modifiers', { ctrl: true }], ['input', 'j'], ['end'], ['modifiers', {}],
    ]);
    assert.deepEqual(receivedInput().slice(plain), ['j'], 'modifier changes apply to the next composition');
  });
  await t.test('a previous commit is retained while the next composition is still active', async () => {
    const before = receivedInput().length;
    await compositionSequence([
      ['modifiers', { ctrl: true }], ['start'], ['update', 'j'], ['wait'], ['end'],
      ['modifiers', {}], ['start'], ['update', 'jk', 'k'], ['wait'], ['end'],
    ]);
    assert.deepEqual(receivedInput().slice(before), ['\n', 'k']);
  });
  await t.test('same-turn composition commits keep their separate modifier snapshots', async () => {
    const before = receivedInput().length;
    await compositionSequence([
      ['modifiers', { ctrl: true }], ['start'], ['update', 'j'], ['end'],
      ['modifiers', {}], ['start'], ['update', 'jk', 'k'], ['end'],
    ]);
    assert.deepEqual(receivedInput().slice(before), ['\n', 'k']);
  });
  await t.test('compositionend before final insertText still produces one Ctrl+J', async () => {
    const before = receivedInput().length;
    await compositionSequence([
      ['modifiers', { ctrl: true }], ['start'], ['update', 'j'], ['wait'], ['end'], ['input', 'j'], ['modifiers', {}],
    ]);
    assert.deepEqual(receivedInput().slice(before), ['\n']);
  });
  await t.test('accessory Enter, Escape and paste finalize Unicode without waiting for native compositionend', async () => {
    for (const [type, value, expected] of [['key', 'Enter', '\r'], ['key', 'Escape', '\x1b'], ['paste', 'pasted', 'pasted']]) {
      const before = receivedInput().length;
      await compositionSequence([['modifiers', {}], ['start'], ['update', '日本語'], ['wait'], [type, value]]);
      assert.deepEqual(receivedInput().slice(before), ['日本語', expected], 'accessory must not wait for IME completion');
      await page.evaluate(() => {
        const textarea = document.querySelector('.xterm-helper-textarea');
        textarea.dispatchEvent(new CompositionEvent('compositionend', { bubbles: true, data: '日本語' }));
        textarea.dispatchEvent(new InputEvent('input', { bubbles: true, inputType: 'insertText', data: '日本語' }));
      });
      await page.waitForTimeout(30);
      assert.deepEqual(receivedInput().slice(before), ['日本語', expected], 'late end plus final input must not duplicate text');
      await page.keyboard.insertText('日本語');
      await page.keyboard.insertText('after');
      await expect.poll(() => receivedInput().slice(before)).toEqual(['日本語', expected, '日本語', 'after']);
    }
  });
  await t.test('hardware Enter, Escape and Backspace keep finalized Unicode before the encoded key', async () => {
    for (const [name, encoded] of [['Enter', '\r'], ['Escape', '\x1b'], ['Backspace', '\x7f']]) {
      const before = receivedInput().length;
      await compositionSequence([['modifiers', {}], ['start'], ['update', '日本語'], ['wait']]);
      await page.keyboard.press(name);
      await expect.poll(() => receivedInput().slice(before)).toEqual(['日本語', encoded]);
      await page.evaluate(() => {
        const textarea = document.querySelector('.xterm-helper-textarea');
        textarea.dispatchEvent(new CompositionEvent('compositionend', { bubbles: true, data: '日本語' }));
        textarea.dispatchEvent(new InputEvent('input', { bubbles: true, inputType: 'insertText', data: '日本語' }));
      });
      await page.waitForTimeout(30);
      assert.deepEqual(receivedInput().slice(before), ['日本語', encoded]);
    }
  });
  await t.test('sticky Ctrl hardware Enter completes modified ASCII without leaking its literal letter', async () => {
    const before = receivedInput().length;
    await compositionSequence([['modifiers', { ctrl: true }], ['start'], ['update', 'j'], ['wait']]);
    await page.keyboard.press('Enter');
    await expect.poll(() => receivedInput().slice(before)).toEqual(['\n', '\r']);
    await page.evaluate(() => document.querySelector('.xterm-helper-textarea')
      .dispatchEvent(new CompositionEvent('compositionend', { bubbles: true, data: 'j' })));
    await page.waitForTimeout(30);
    assert.deepEqual(receivedInput().slice(before), ['\n', '\r']);
    await send({ type: 'modifiers', modifiers: noModifiers });
  });
  await t.test('Unicode composition and cancellation remain literal with or without sticky Ctrl', async () => {
    for (const modifiers of [{}, { ctrl: true }]) {
      const before = receivedInput().length;
      await compositionSequence([
        ['modifiers', modifiers], ['start'], ['update', '日本語'], ['input', '日本語'],
        ['end', 'incorrect event data'], ['wait'], ['start'], ['update', 'cancelled'], ['update', ''], ['end'], ['modifiers', {}],
      ]);
      assert.deepEqual(receivedInput().slice(before), ['日本語']);
    }
  });
  await t.test('reset cancels pending modified IME input instead of sending it to a new epoch', async () => {
    const before = receivedInput().length;
    const replacement = 'keyboard-reset';
    await compositionSequence([
      ['modifiers', { ctrl: true }], ['start'], ['update', 'j'], ['end'], ['key', 'Escape'], ['reset', replacement],
    ]);
    epoch = replacement;
    assert.deepEqual(receivedInput().slice(before), []);
    await output('REPLAY_SCREEN', true);
    await send({ type: 'modifiers', modifiers: noModifiers });
    await page.keyboard.insertText('after');
    await expect.poll(() => receivedInput().slice(before)).toEqual(['after']);
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
  await t.test('swipes scroll restored history and stay there during live output without sending input', async () => {
    epoch = 'history';
    await send({ type: 'reset', cols: 80, rows: 24, fontSize: 14 });
    await send({ type: 'focus', focused: false });
    await output(Array.from({ length: 150 }, (_, i) => `HISTORY_${String(i).padStart(3, '0')}\r\n`).join(''), true, true);
    await expect(page.locator('.xterm-rows')).toContainText('HISTORY_149');
    const before = receivedInput().length;
    await swipe(180);
    await expect(page.locator('.xterm-rows')).not.toContainText('HISTORY_149');
    const scrolled = await page.locator('.xterm-rows').textContent();
    await output('LIVE_WHILE_READING\r\n');
    assert.equal(await page.locator('.xterm-rows').textContent(), scrolled);
    await swipe(-240);
    await expect(page.locator('.xterm-rows')).toContainText('LIVE_WHILE_READING');
    await swipe(100, true);
    await swipe(-200);
    await expect(page.locator('.xterm-rows')).toContainText('LIVE_WHILE_READING');
    assert.equal(receivedInput().length, before, 'scrolling must never write terminal input');
    await expect(page.locator('.xterm-helper-textarea')).not.toBeFocused();
  });
  async function swipe(distance, cancel = false) {
    return page.evaluate(({ distance, cancel }) => {
      const screen = document.querySelector('.xterm-screen'), rect = screen.getBoundingClientRect();
      const x = rect.left + rect.width / 2, y = rect.top + rect.height / 2;
      for (const [type, offset] of [['touchstart', 0], ['touchmove', distance], [cancel ? 'touchcancel' : 'touchend', distance]]) {
        const touch = new Touch({ identifier: 1, target: screen, clientX: x, clientY: y + offset });
        screen.dispatchEvent(new TouchEvent(type, { bubbles: true, cancelable: true,
          touches: type === 'touchstart' || type === 'touchmove' ? [touch] : [], changedTouches: [touch] }));
      }
    }, { distance, cancel });
  }
  await t.test('mouse-enabled agent views receive swipe wheel events in both directions after attach', async () => {
    for (const mode of [1000, 1002, 1003]) {
      epoch = `agent-${mode}`;
      await send({ type: 'reset', cols: 80, rows: 24, fontSize: 14 });
      // A restored Pi-style screen includes both alternate-buffer and mouse modes.
      await output(`\x1b[?1049h\x1b[?${mode}h\x1b[?1006hAGENT_VIEW`, true, true);
      const before = receivedInput().length;
      await swipe(100);
      await expect.poll(() => receivedInput().slice(before).length).toBeGreaterThan(0);
      assert.ok(receivedInput().slice(before).every(data => /^\x1b\[<64;\d+;\d+M$/.test(data)));
      const down = receivedInput().length;
      await swipe(-100);
      await expect.poll(() => receivedInput().slice(down).length).toBeGreaterThan(0);
      assert.ok(receivedInput().slice(down).every(data => /^\x1b\[<65;\d+;\d+M$/.test(data)));
      await expect(page.locator('.xterm-helper-textarea')).not.toBeFocused();
      const selected = receivedInput().length;
      await send({ type: 'select', enabled: true });
      await swipe(100);
      assert.equal(receivedInput().length, selected, 'selection must not send mouse reports');
      await send({ type: 'select', enabled: false });
      await output(`\x1b[?${mode}l\x1b[?1006l\x1b[?1049l`);
    }
  });
  await t.test('alternate-screen swipes do not invent remote input', async () => {
    await output('\x1b[?1049hALT_SCREEN');
    const before = receivedInput().length;
    await page.evaluate(() => {
      const screen = document.querySelector('.xterm-screen');
      for (const [type, y] of [['touchstart', 100], ['touchmove', 200], ['touchend', 200]]) {
        const touch = new Touch({ identifier: 1, target: screen, clientX: 100, clientY: y });
        screen.dispatchEvent(new TouchEvent(type, { bubbles: true, cancelable: true,
          touches: type === 'touchend' ? [] : [touch], changedTouches: [touch] }));
      }
    });
    assert.equal(receivedInput().length, before);
    await output('\x1b[?9h\x1b[?1006h');
    await swipe(100);
    assert.equal(receivedInput().length, before, 'X10 tracking does not support wheel reports');
    await output('\x1b[?9l\x1b[?1006l\x1b[?1049l');
  });
  await t.test('touch selection emits text for the clipboard action', async () => {
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
  await t.test('ownership loss removes old contents and textarea without fitting or sending input', async () => {
    epoch = 'owner-before-loss';
    await send({ type: 'reset', cols: 115, rows: 32, fontSize: 14 });
    await output('PRIVATE_OWNER_CONTENTS', true, true);
    await expect(page.locator('.xterm-rows')).toContainText('PRIVATE_OWNER_CONTENTS');
    epoch = 'blocked-view'; await send({ type: 'clear' });
    assert.equal(await page.locator('.xterm-rows, .xterm-helper-textarea').count(), 0);
    const inputs = receivedInput().length, sizes = messages.filter(message => message.type === 'resize').length;
    await page.setViewportSize({ width: 420, height: 500 });
    await send({ type: 'output', epoch: 'owner-before-loss', ticket: 800, data: 'STALE_PRIVATE_CONTENTS', historical: false, fit: true });
    await page.evaluate(() => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve))));
    assert.equal(messages.filter(message => message.type === 'resize').length, sizes);
    assert.equal(receivedInput().length, inputs);
    assert.ok(!messages.some(message => message.type === 'rendered' && message.ticket === 800));
    epoch = 'owner-after-take';
    await send({ type: 'reset', cols: 115, rows: 32, fontSize: 14 });
    await output('FRESH_OWNER_SNAPSHOT', true, true);
    await expect(page.locator('.xterm-rows')).toContainText('FRESH_OWNER_SNAPSHOT');
    await expect(page.locator('.xterm-rows')).not.toContainText('PRIVATE_OWNER_CONTENTS');
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
  await terminalInputCases(t, { page, send, output, receivedInput });
  assert.deepEqual(errors, []);
});
