import assert from 'node:assert/strict';
import test from 'node:test';
import { controlBounds, keyboardIsShown, needsKeyboardDismissal, revealControl, visibleControl } from './mobile-e2e-controls.mjs';

const label = 'Use background #ffffff';
const clipped = { label, text: '', bounds: '[0,0][0,0]' };
const visible = { label, text: '', bounds: '[30,220][58,248]' };

test('only positive drawable bounds can be tapped', () => {
  assert.deepEqual(controlBounds(visible), { left: 30, top: 220, right: 58, bottom: 248 });
  for (const bounds of ['', '[0,0][0,0]', '[10,20][10,40]', '[10,20][30,20]', '[30,20][10,40]', '[-1,20][30,40]', 'garbage']) {
    assert.equal(controlBounds({ bounds }), undefined, bounds);
  }
  assert.equal(controlBounds(undefined), undefined);
});

test('visible control lookup ignores clipped matches and prefers accessibility labels', () => {
  const textOnly = { text: label, label: '', bounds: '[1,1][20,20]' };
  assert.equal(visibleControl([clipped], label), undefined);
  assert.equal(visibleControl([clipped, textOnly, visible], label), visible);
  assert.equal(visibleControl([textOnly], label), textOnly);
});

test('a color present below the fold must be scrolled into view before being returned', async () => {
  let scrolls = 0;
  const result = await revealControl(label, {
    read: () => [scrolls ? visible : clipped],
    scroll: direction => { assert.equal(direction, 'forward'); scrolls++; },
  });
  assert.equal(result, visible);
  assert.equal(scrolls, 1);
});

test('an already visible choice does not move the screen', async () => {
  assert.equal(await revealControl(label, { read: () => [visible], scroll: () => assert.fail('unexpected scrolling') }), visible);
});

test('restoring a color above the current position searches backward too', async () => {
  let position = 1;
  const directions = [];
  const result = await revealControl(label, {
    read: () => [position === 0 ? visible : clipped],
    scroll: direction => {
      directions.push(direction);
      position = Math.max(0, Math.min(2, position + (direction === 'forward' ? 1 : -1)));
    },
    attempts: 2,
  });
  assert.equal(result, visible);
  assert.deepEqual(directions, ['forward', 'forward', 'backward', 'backward']);
});

test('keyboard visibility avoids Back when the IME is already hidden', () => {
  assert.equal(keyboardIsShown('  mInputShown=true\n  mIsInputViewShown=true mStatusIcon=0'), true);
  assert.equal(keyboardIsShown('  mInputShown=false\n  mIsInputViewShown=false mStatusIcon=0'), false);
  assert.equal(keyboardIsShown('  mInputShown=false\n  mIsInputViewShown=true mStatusIcon=0'), false);
  assert.equal(keyboardIsShown('  mIsInputViewShown=true mStatusIcon=0'), true);
  assert.equal(keyboardIsShown('unrecognized Android dump'), false);
});

test('a floating visible IME must not trigger Back when Save is already drawable', () => {
  const shown = '  mInputShown=true\n  mIsInputViewShown=true mStatusIcon=0';
  assert.equal(needsKeyboardDismissal(visible, shown), false);
  assert.equal(needsKeyboardDismissal(clipped, shown), true);
  assert.equal(needsKeyboardDismissal(undefined, '  mInputShown=false'), false);
});

test('unreachable choices fail instead of tapping zero-area nodes or scrolling forever', async () => {
  let scrolls = 0;
  await assert.rejects(revealControl(label, { read: () => [clipped], scroll: () => { scrolls++; } }), /Could not reach/);
  assert.equal(scrolls, 12);
});
