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

  const erase = async (inputType = 'deleteContentBackward') => page.evaluate(async inputType => {
    const textarea = document.querySelector('.xterm-helper-textarea');
    textarea.dispatchEvent(new KeyboardEvent('keydown', { bubbles: true, key: 'Unidentified', keyCode: 229 }));
    textarea.dispatchEvent(new InputEvent('beforeinput', { bubbles: true, inputType, data: null }));
    const start = textarea.selectionStart, end = textarea.selectionEnd;
    const from = start === end && inputType === 'deleteContentBackward' ? Math.max(0, start - 1) : start;
    const to = start === end && inputType === 'deleteContentForward' ? Math.min(textarea.value.length, end + 1) : end;
    textarea.setRangeText('', from, to, 'end');
    textarea.dispatchEvent(new InputEvent('input', { bubbles: true, inputType, data: null }));
    textarea.dispatchEvent(new KeyboardEvent('keyup', { bubbles: true, key: 'Unidentified', keyCode: 229 }));
    await new Promise(resolve => setTimeout(resolve, 25));
  }, inputType);
  const encodedKey = async (key, accessory) => {
    if (accessory) await send({ type: 'key', event: { key, modifiers: { ctrl: false, alt: false, shift: false, meta: false, capsLock: false } } });
    else await page.keyboard.press(key);
  };

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
  await t.test('encoded Backspace invalidates retained context before native typing', async () => {
    for (const accessory of [false, true]) {
      await fresh(); await seed('abc'); const before = receivedInput().length;
      await encodedKey('Backspace', accessory);
      await expect(page.locator('.xterm-helper-textarea')).toHaveValue('');
      await edit({ data: 'd' });
      await expect.poll(() => receivedInput().slice(before)).toEqual(['\x7f', 'd']);
    }
  });
  await t.test('native Backspace keeps deleting remote text after encoded Backspace exhausts local context', async () => {
    for (const accessory of [false, true]) {
      await fresh(); await seed('abc'); const before = receivedInput().length;
      await encodedKey('Backspace', accessory);
      await edit({ data: 'd' });
      await erase(); await erase();
      await expect.poll(() => receivedInput().slice(before)).toEqual(['\x7f', 'd', '\x7f', '\x7f']);
    }
  });
  await t.test('encoded arrows invalidate old context before a native replacement at the new remote cursor', async () => {
    for (const accessory of [false, true]) {
      await fresh(); await seed('abc'); const before = receivedInput().length;
      await encodedKey('ArrowLeft', accessory);
      await expect(page.locator('.xterm-helper-textarea')).toHaveValue('');
      await edit({ data: 'x' });
      await edit({ data: 'X', inputType: 'insertReplacementText', start: 0, end: 1 });
      await expect.poll(() => receivedInput().slice(before)).toEqual(['\x1b[D', 'x', '\x7fX']);
    }
  });
  await t.test('native forward deletion at an encoded-arrow position reaches the remote terminal', async () => {
    await fresh(); await seed('abc'); const before = receivedInput().length;
    await encodedKey('ArrowLeft', false);
    await erase('deleteContentForward');
    await expect.poll(() => receivedInput().slice(before)).toEqual(['\x1b[D', '\x1b[3~']);
  });
  await t.test('native Backspace with no retained textarea text still erases remote text', async () => {
    await fresh(); const before = receivedInput().length;
    await erase();
    await expect.poll(() => receivedInput().slice(before)).toEqual(['\x7f']);
  });
  await t.test('accessory paste retires pre-paste native context before the next edit', async () => {
    await fresh(); await seed('abc'); const before = receivedInput().length;
    await send({ type: 'paste', data: 'pasted' });
    await expect(page.locator('.xterm-helper-textarea')).toHaveValue('');
    await edit({ data: 'x' });
    await erase(); await erase();
    await expect.poll(() => receivedInput().slice(before)).toEqual(['pasted', 'x', '\x7f', '\x7f']);
  });
  await t.test('an unchanged replacement moves the remote cursor to the native replacement end', async () => {
    for (const composing of [false, true]) {
      await fresh(); await seed('abc def ghi'); const before = receivedInput().length;
      await edit({ data: 'def', inputType: composing ? 'insertCompositionText' : 'insertReplacementText', start: 4, end: 7, composing });
      await expect.poll(() => receivedInput().slice(before)).toEqual(['\x1b[D'.repeat(4)]);
    }
  });
  await t.test('composition replacing a middle word starts from the committed remote cursor, not the selected range', async () => {
    await fresh(); await seed('abc def ghi'); const before = receivedInput().length;
    await edit({ data: 'DEF', inputType: 'insertCompositionText', start: 4, end: 7, composing: true });
    await expect.poll(() => receivedInput().slice(before)).toEqual(['\x1b[D'.repeat(4) + '\x7f'.repeat(3) + 'DEF']);
  });
  await t.test('a late final DOM insertion after forced composition completion does not replay the commit', async () => {
    await fresh(); const before = receivedInput().length;
    await page.evaluate(() => {
      const textarea = document.querySelector('.xterm-helper-textarea');
      textarea.dispatchEvent(new CompositionEvent('compositionstart', { bubbles: true }));
      textarea.dispatchEvent(new InputEvent('beforeinput', { bubbles: true, inputType: 'insertCompositionText', data: '日本語', isComposing: true }));
      textarea.setRangeText('日本語', 0, 0, 'end');
      textarea.dispatchEvent(new CompositionEvent('compositionupdate', { bubbles: true, data: '日本語' }));
      textarea.dispatchEvent(new InputEvent('input', { bubbles: true, inputType: 'insertCompositionText', data: '日本語', isComposing: true }));
    });
    await encodedKey('Enter', true);
    await expect.poll(() => receivedInput().slice(before)).toEqual(['日本語', '\r']);
    await page.evaluate(async () => {
      const textarea = document.querySelector('.xterm-helper-textarea');
      textarea.dispatchEvent(new CompositionEvent('compositionend', { bubbles: true, data: '日本語' }));
      textarea.dispatchEvent(new InputEvent('beforeinput', { bubbles: true, inputType: 'insertText', data: '日本語' }));
      textarea.setRangeText('日本語', textarea.selectionStart, textarea.selectionEnd, 'end');
      textarea.dispatchEvent(new InputEvent('input', { bubbles: true, inputType: 'insertText', data: '日本語' }));
      await new Promise(resolve => setTimeout(resolve, 25));
    });
    await expect.poll(() => receivedInput().slice(before)).toEqual(['日本語', '\r']);
  });
  await t.test('middle-word Unicode replacements preserve complete characters and application cursor encoding', async () => {
    for (const applicationCursor of [false, true]) {
      for (const composing of [false, true]) {
        await fresh(); await output(applicationCursor ? '\x1b[?1h' : '\x1b[?1l');
        await seed('a🙂e\u0301z'); const before = receivedInput().length;
        await edit({ data: '🙃é', inputType: composing ? 'insertCompositionText' : 'insertReplacementText', start: 1, end: 5, composing });
        await expect.poll(() => receivedInput().slice(before)).toEqual([(applicationCursor ? '\x1bOD' : '\x1b[D') + '\x7f'.repeat(2) + '🙃é']);
      }
    }
  });
  await t.test('native deletion of an emoji or combining sequence emits a single erase', async () => {
    for (const text of ['🙂', 'e\u0301']) {
      await fresh(); await seed('a' + text); const before = receivedInput().length;
      await edit({ data: null, inputType: 'deleteContentBackward', start: 1, end: 1 + text.length });
      await expect.poll(() => receivedInput().slice(before)).toEqual(['\x7f']);
    }
  });
  await t.test('autocorrect, punctuation, selection deletion, undo and redo preserve a remote line through successive edits', async () => {
    for (const composing of [false, true]) {
      await fresh(); await seed('teh cat sat');
      let line = 'teh cat sat', cursor = line.length;
      const changes = [
        { start: 0, end: 3, data: 'the', inputType: 'insertReplacementText' },
        { start: 4, end: 7, data: 'kitten', inputType: 'insertReplacementText' },
        { start: 4, end: 10, data: 'cat', inputType: 'insertReplacementText' },
        { start: 7, end: 8, data: '. ', inputType: 'insertReplacementText' },
        { start: 0, end: 1, data: 'T', inputType: 'insertReplacementText' },
        { start: 2, end: 6, data: null, inputType: 'deleteContentBackward' },
        { start: 0, end: 1, data: null, inputType: 'deleteContentForward' },
        { start: 0, end: 1, data: 'he ca', inputType: 'historyUndo' },
        { start: 0, end: 5, data: 'h', inputType: 'historyRedo' },
        { start: 1, end: 1, data: '!?', inputType: 'insertText' },
      ];
      for (const change of changes) {
        const before = receivedInput().length;
        await edit({ ...change, composing });
        const expected = line.slice(0, change.start) + (change.data ?? '') + line.slice(change.end);
        const expectedCursor = change.start + (change.data ?? '').length;
        const input = receivedInput().slice(before).join('');
        for (let i = 0; i < input.length; i++) {
          if (input[i] === '\x1b') { cursor += input[i + 2] === 'D' ? -1 : 1; i += 2; }
          else if (input[i] === '\x7f') { line = line.slice(0, cursor - 1) + line.slice(cursor); cursor--; }
          else { line = line.slice(0, cursor) + input[i] + line.slice(cursor); cursor++; }
          assert.ok(cursor >= 0 && cursor <= line.length, 'remote caret must stay inside the line');
        }
        assert.deepEqual({ line, cursor }, { line: expected, cursor: expectedCursor }, JSON.stringify({ change, composing, input }));
      }
      // Backspace must empty the whole retained context, including its last symbol.
      const remaining = line.length;
      for (let count = remaining; count > 0; count--) {
        const before = receivedInput().length;
        await edit({ data: null, inputType: 'deleteContentBackward', start: line.length - 1, end: line.length, composing });
        const input = receivedInput().slice(before).join('');
        for (let i = 0; i < input.length; i++) {
          if (input[i] === '\x1b') { cursor += input[i + 2] === 'D' ? -1 : 1; i += 2; }
          else if (input[i] === '\x7f') { line = line.slice(0, cursor - 1) + line.slice(cursor); cursor--; }
          else assert.fail('deletion must not reinsert old context');
        }
      }
      assert.deepEqual({ line, cursor }, { line: '', cursor: 0 });
    }
  });
  await t.test('IME deletion emits one erase, not a copy of its remaining buffer', async () => {
    await fresh(); await seed('phrase'); const before = receivedInput().length;
    await edit({ data: null, inputType: 'deleteContentBackward', start: 5, end: 6 });
    await expect.poll(() => receivedInput().slice(before)).toEqual(['\x7f']);
  });
}
