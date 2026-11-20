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

test('selects an exact AppKit row in the preconfigured directory and waits for Open', async () => {
  const name = 'fixture.txt';
  let visible = true;
  let selected = false;
  let reads = 0;
  const picker = nativePicker((action, value) => {
    if (action === 'windows') return visible ? [desktop, openPanel, cancel,
      { role: 'AXStaticText', value: name }, { role: 'AXButton', title: 'Open', enabled: selected && ++reads > 1 },
    ] : [desktop];
    if (action === 'select-entry') {
      assert.equal(value, name);
      selected = true;
      return;
    }
    assert.deepEqual([action, value], ['press', 'Open']);
    assert.ok(reads > 1);
    visible = false;
  });
  await picker.choose(name);
  assert.equal(visible, false);
});
