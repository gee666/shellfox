import assert from 'node:assert/strict';
import test from 'node:test';
import { nativePicker } from './macos-p4-dialog.mjs';

const openPanel = { role: 'AXWindow', subrole: 'AXStandardWindow', title: 'Open' };
const cancel = { role: 'AXButton', title: 'Cancel' };

test('recognizes and cancels the standalone AppKit panel', async () => {
  let visible = true;
  const picker = nativePicker((action, label) => {
    if (action === 'dialogs') return visible ? [openPanel, cancel] : [];
    assert.deepEqual([action, label], ['press', 'Cancel']);
    visible = false;
  });
  const rows = await picker.wait();
  assert.ok(rows.includes(openPanel));
  await picker.cancel();
  assert.equal(visible, false);
});

test('waits for the owned window after a transient accessibility failure', async () => {
  let reads = 0;
  const picker = nativePicker(action => {
    assert.equal(action, 'dialogs');
    if (++reads === 1) throw new Error('No accessible window from the owned fixture');
    return [openPanel, cancel];
  });
  assert.deepEqual(await picker.wait(), [openPanel, cancel]);
  assert.equal(reads, 2);
});

test('selects an exact AppKit row in the preconfigured directory and waits for Open', async () => {
  const name = 'fixture.txt';
  let visible = true;
  let selected = false;
  let reads = 0;
  const picker = nativePicker((action, value) => {
    if (action === 'dialogs') return visible ? [openPanel, cancel,
      { role: 'AXStaticText', value: name }, { role: 'AXButton', title: 'Open', enabled: selected && ++reads > 1 },
    ] : [];
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

test('verifies the real overwrite sheet when AppKit does not acknowledge Save', async () => {
  const name = 'fixture.txt';
  let step = 'save';
  const picker = nativePicker((action, value) => {
    if (action === 'dialogs') {
      if (step === 'save') return [{ role: 'AXTextField', value: name }];
      return step === 'confirm' ? [{ role: 'AXButton', title: 'Replace' }] : [];
    }
    assert.equal(action, 'press');
    if (value === 'Save') { step = 'confirm'; throw new Error('AXPress could not complete'); }
    assert.equal(value, 'Replace');
    step = 'closed';
  });
  await picker.save(name);
  await picker.replace(true);
  assert.equal(step, 'closed');
});
