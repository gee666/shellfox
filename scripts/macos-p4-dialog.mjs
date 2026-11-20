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
  const read = () => action('windows');
  const cancelButton = rows => rows.some(row => row.role === 'AXButton' && label(row) === 'Cancel');
  const dialog = rows => cancelButton(rows) && rows.some(row =>
    row.subrole === 'AXDialog' || row.role === 'AXDialog' || row.role === 'AXSheet' ||
    (row.role === 'AXWindow' && ['Open', 'Save', 'Save As'].includes(row.title)));
  const dismissed = () => eventually('native picker dismissed', read, rows => rows.length > 0 && !dialog(rows));
  const choose = async name => {
    await eventually(`native picker lists ${name}`, read, rows => rows.some(row => label(row) === name || row.value === name));
    action('select-entry', name);
    await eventually('native Open enabled', read, rows => rows.some(row => row.role === 'AXButton' && label(row) === 'Open' && row.enabled));
    action('press', 'Open');
    await dismissed();
  };
  return {
    wait: () => eventually('owned native file dialog', read, dialog),
    cancel: async () => { action('press', 'Cancel'); await dismissed(); },
    choose,
    async save(name) {
      await eventually('sanitize and prefill download filename', read, rows => rows.some(row => row.role === 'AXTextField' && row.value === name));
      action('press', 'Save');
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
