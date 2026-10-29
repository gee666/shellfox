import assert from 'node:assert/strict';
import { expect } from '@playwright/test';

/** Runs real DOM event transactions against the bundled xterm, not mocked onData calls. */
export async function terminalInputCases(t, { page, send, output, receivedInput }) {
  const fresh = async () => {
    await send({ type: 'reset', cols: 80, rows: 24, fontSize: 14 });
    await output('', true);
    await send({ type: 'focus', focused: true });
  };
  const edit = async ({ data, inputType = 'insertText', start, end, composing = false, keys = 1, composed = true }) => page.evaluate(async args => {
    const textarea = document.querySelector('.xterm-helper-textarea');
    for (let i = 0; i < args.keys; i++) textarea.dispatchEvent(new KeyboardEvent('keydown', { bubbles: true, composed: true, key: 'Unidentified', keyCode: 229 }));
    const start = args.start ?? textarea.selectionStart, end = args.end ?? textarea.selectionEnd;
    textarea.setSelectionRange(start, end);
    if (args.composing) textarea.dispatchEvent(new CompositionEvent('compositionstart', { bubbles: true }));
    textarea.dispatchEvent(new InputEvent('beforeinput', { bubbles: true, composed: args.composed, inputType: args.inputType, data: args.data, isComposing: args.composing }));
    textarea.setRangeText(args.data ?? '', start, end, 'end');
    if (args.composing) textarea.dispatchEvent(new CompositionEvent('compositionupdate', { bubbles: true, data: args.data }));
    textarea.dispatchEvent(new InputEvent('input', { bubbles: true, composed: args.composed, inputType: args.inputType, data: args.data, isComposing: args.composing }));
    if (args.composing) textarea.dispatchEvent(new CompositionEvent('compositionend', { bubbles: true, data: args.data }));
    textarea.dispatchEvent(new KeyboardEvent('keyup', { bubbles: true, composed: true, key: 'Unidentified', keyCode: 229 }));
    await new Promise(resolve => setTimeout(resolve, 25));
  }, { data, inputType, start, end, composing, keys, composed });

  const seed = async text => {
    const before = receivedInput().length;
    await page.keyboard.insertText(text);
    await expect.poll(() => receivedInput().slice(before)).toEqual([text]);
  };

  await t.test('repeated Android 229 key events do not replay a whole phrase', async () => {
    await fresh(); const before = receivedInput().length;
    await edit({ data: "look this is typed twice. I didn't do it", keys: 2 });
    await expect.poll(() => receivedInput().slice(before)).toEqual(["look this is typed twice. I didn't do it"]);
  });
  await t.test('insertText has one owner even when input is not composed', async () => {
    await fresh(); const before = receivedInput().length;
    await edit({ data: '1', composed: false });
    await expect.poll(() => receivedInput().slice(before)).toEqual(['1']);
  });
  await t.test('equal-length autocorrect replaces text instead of reinserting its retained context', async () => {
    for (const composing of [false, true]) {
      await fresh(); await seed('there is one more thing');
      const before = receivedInput().length;
      await edit({ data: 'Thing', inputType: composing ? 'insertCompositionText' : 'insertReplacementText', start: 18, end: 23, composing });
      await expect.poll(() => receivedInput().slice(before), { message: `autocorrect composing=${composing}` })
        .toEqual(['\x1b[D'.repeat(4) + '\x7fT' + '\x1b[C'.repeat(4)]);
    }
  });
  await t.test('legitimate identical phrases, digits and Unicode commits are never deduplicated', async () => {
    await fresh(); const before = receivedInput().length;
    for (let i = 0; i < 2; i++) await edit({ data: 'one more thing ' });
    for (let i = 0; i < 3; i++) await edit({ data: '1' });
    for (let i = 0; i < 2; i++) await edit({ data: '日本語🙂', composing: true, inputType: 'insertCompositionText' });
    await expect.poll(() => receivedInput().slice(before)).toEqual(['one more thing ', 'one more thing ', '1', '1', '1', '日本語🙂', '日本語🙂']);
  });
  await t.test('Gboard per-character beforeinput/input traces preserve every character under timer pressure', async () => {
    await fresh(); const before = receivedInput().length;
    const phrase = 'there is one more thing there is one more thing look this is typed twice ';
    await page.evaluate(async phrase => {
      const textarea = document.querySelector('.xterm-helper-textarea');
      for (const data of phrase) {
        textarea.dispatchEvent(new KeyboardEvent('keydown', { bubbles: true, key: 'Unidentified', keyCode: 229 }));
        textarea.dispatchEvent(new InputEvent('beforeinput', { bubbles: true, inputType: 'insertText', data }));
        textarea.setRangeText(data, textarea.selectionStart, textarea.selectionEnd, 'end');
        textarea.dispatchEvent(new InputEvent('input', { bubbles: true, inputType: 'insertText', data }));
        textarea.dispatchEvent(new KeyboardEvent('keyup', { bubbles: true, key: 'Unidentified', keyCode: 229 }));
        await Promise.resolve();
      }
      await new Promise(resolve => setTimeout(resolve, 25));
    }, phrase);
    await expect.poll(() => receivedInput().slice(before)).toEqual([...phrase]);
  });
  await t.test('native clipboard paste has one owner and retains bracketed paste semantics', async () => {
    await fresh(); await output('\x1b[?2004h'); const before = receivedInput().length;
    const prevented = await page.evaluate(() => {
      const clipboardData = new DataTransfer(); clipboardData.setData('text/plain', 'clipboard\n日本語');
      const event = new ClipboardEvent('paste', { bubbles: true, cancelable: true, clipboardData });
      document.querySelector('.xterm-helper-textarea').dispatchEvent(event);
      return event.defaultPrevented;
    });
    assert.equal(prevented, true, 'the browser must not insert a second copy after xterm paste');
    await expect.poll(() => receivedInput().slice(before)).toEqual(['\x1b[200~clipboard\r日本語\x1b[201~']);
  });
  await t.test('Gboard insertFromPaste without a clipboard event is bracketed once per intentional paste', async () => {
    await fresh(); await output('\x1b[?2004h'); const before = receivedInput().length;
    await edit({ data: 'native\n日本語', inputType: 'insertFromPaste' });
    await edit({ data: 'native\n日本語', inputType: 'insertFromPaste' });
    await expect.poll(() => receivedInput().slice(before)).toEqual(['\x1b[200~native\r日本語\x1b[201~', '\x1b[200~native\r日本語\x1b[201~']);
  });
  await t.test('IME deletion emits one erase, not a copy of its remaining buffer', async () => {
    await fresh(); await seed('phrase'); const before = receivedInput().length;
    await edit({ data: null, inputType: 'deleteContentBackward', start: 5, end: 6 });
    await expect.poll(() => receivedInput().slice(before)).toEqual(['\x7f']);
  });
}
