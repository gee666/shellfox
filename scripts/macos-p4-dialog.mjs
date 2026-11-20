import assert from 'node:assert/strict';
import { setTimeout as delay } from 'node:timers/promises';

export async function eventually(label, read, predicate, ms = 20_000) {
  const deadline = Date.now() + ms;
  let last;
  while (Date.now() < deadline) {
    last = await read();
    if (predicate(last)) return last;
    await delay(300);
  }
  assert.fail(`${label}: ${JSON.stringify(last)?.slice(0, 4000)}`);
}

const label = row => row.title || row.description;

export function nativePicker(action) {
  const key = (code, modifiers = '-') => action('dialog-key', code, modifiers);
  const read = () => action('windows');
  const cancelButton = rows => rows.some(row => row.role === 'AXButton' && label(row) === 'Cancel');
  const dialog = rows => cancelButton(rows) && rows.some(row =>
    row.subrole === 'AXDialog' || row.role === 'AXDialog' || row.role === 'AXSheet');
  const dismissed = () => eventually('native picker dismissed', read, rows => !dialog(rows));
  const textEntry = rows => rows.find(row => row.focused && ['AXTextField', 'AXComboBox'].includes(row.role));
  const go = async target => {
    key(44); // Typing / in an open/save panel starts Go to Folder.
    await eventually('native Go to Folder input', read, rows => Boolean(textEntry(rows)));
    key(0, 'c');
    action('dialog-paste', target);
    const entered = rows => textEntry(rows)?.value?.replace(/\/$/, '') === target;
    await eventually('native picker received the fixture path', read, entered);
    key(36);
    await eventually('native Go to Folder sheet closed', read, rows => !entered(rows));
  };
  return {
    wait: () => eventually('owned native file dialog', read, dialog),
    cancel: async () => { action('press', 'Cancel'); await dismissed(); },
    choose: async target => { await go(target); key(36); await dismissed(); },
    async uploadFrom(directory, filename) {
      await go(directory);
      await eventually('fixture file is in the native upload picker', read, rows => rows.some(row => label(row) === filename || row.value === filename));
      // Each fixture upload directory holds exactly one file.
      key(0, 'c');
      action('press', 'Open');
      await dismissed();
    },
    async downloadTo(directory, name) {
      await go(directory);
      await eventually('sanitize and prefill download filename', read, rows => rows.some(row => row.value === name));
      key(36);
    },
    async replace(accept) {
      const confirmation = rows => rows.some(row => row.role === 'AXButton' && label(row) === 'Replace');
      await eventually('native overwrite confirmation', read, confirmation);
      if (accept) action('press', 'Replace');
      else {
        action('press', 'Cancel');
        const remaining = await eventually('overwrite confirmation dismissed', read, rows => !confirmation(rows));
        if (dialog(remaining)) action('press', 'Cancel');
      }
      await dismissed();
    },
    dismissed,
  };
}
