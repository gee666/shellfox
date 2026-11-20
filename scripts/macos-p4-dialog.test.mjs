import assert from 'node:assert/strict';
import test from 'node:test';
import { nativePicker } from './macos-p4-dialog.mjs';

const desktop = { role: 'AXWindow', title: 'Shellfox' };
const openPanel = { role: 'AXWindow', subrole: 'AXStandardWindow', title: 'Open' };
const cancel = { role: 'AXButton', title: 'Cancel' };

test('recognizes and cancels the standalone AppKit panel', async () => {
  let visible = true;
  const picker = nativePicker((action, label) => {
    if (action === 'windows') return visible ? [desktop, openPanel, cancel] : [desktop];
    assert.deepEqual([action, label], ['press', 'Cancel']);
    visible = false;
  });
  const rows = await picker.wait();
  assert.ok(rows.includes(openPanel));
  await picker.cancel();
  assert.equal(visible, false);
});

test('waits for AppKit to resolve the path before confirming it', async () => {
  const target = '/private/fixture';
  let step = 'panel', path = '', scans = 0;
  const picker = nativePicker((action, argument) => {
    if (action === 'windows') {
      const rows = [desktop];
      if (step !== 'closed') rows.push(openPanel, cancel);
      if (step === 'go') {
        rows.push({ role: 'AXTextField', focused: true, value: path });
        if (path && ++scans > 1) rows.push({ role: 'AXStaticText', value: 'Go to:' });
      }
      return rows;
    }
    if (action === 'dialog-value') {
      path = argument;
      return;
    }
    assert.equal(action, 'dialog-key');
    if (argument === 5) step = 'go';
    else if (step === 'go') {
      assert.ok(scans > 1);
      assert.equal(path, target);
      step = 'panel';
    } else step = 'closed';
  });
  await picker.choose(target);
  assert.equal(step, 'closed');
});
