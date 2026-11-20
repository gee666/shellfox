import assert from 'node:assert/strict';
import path from 'node:path';
import { setTimeout as delay } from 'node:timers/promises';

export async function eventually(label, read, predicate, ms = 20_000) {
  const deadline = Date.now() + ms;
  let last, timeout;
  while (Date.now() < deadline) {
    try {
      last = await read();
      if (predicate(last)) return last;
    } catch (error) {
      if (!String(error).includes('ETIMEDOUT')) throw error;
      timeout = error;
    }
    await delay(300);
  }
  if (last === undefined && timeout) throw timeout;
  assert.fail(`${label}: ${JSON.stringify(last)?.slice(0, 4000)}`);
}

const label = row => row.title || row.description;

export function nativePicker(action) {
  const read = () => action('dialogs');
  const cancelButton = rows => rows.some(row => row.role === 'AXButton' && label(row) === 'Cancel');
  const dialog = rows => cancelButton(rows) && rows.some(row =>
    row.subrole === 'AXDialog' || row.role === 'AXDialog' || row.role === 'AXSheet' ||
    (row.role === 'AXWindow' && ['Open', 'Save', 'Save As'].includes(row.title)));
  const dismissed = () => eventually('native picker dismissed', read, rows => rows.length === 0);
  const confirmation = rows => rows.some(row => row.role === 'AXButton' && label(row) === 'Replace');
  const openEnabled = rows => rows.some(row => row.role === 'AXButton' && label(row) === 'Open' && row.enabled);
  const confirmOpen = async (ms = 20_000) => {
    await eventually('native Open enabled', read, openEnabled, ms);
    try { action('press', 'Open'); }
    catch (error) {
      await dismissed().catch(() => { throw error; });
      return;
    }
    await dismissed();
  };
  const choose = async name => {
    await eventually(`native picker lists ${name}`, read, rows => rows.some(row => label(row) === name || row.value === name));
    action('select-entry', name);
    try { await eventually('native file row selected', read, openEnabled, 5_000); }
    catch (error) {
      if (error?.code !== 'ERR_ASSERTION') throw error;
      // Repeat only the row click, never Open, if the file is still unselected.
      if (!openEnabled(read())) action('select-entry', name);
    }
    await confirmOpen(15_000);
  };
  return {
    wait: () => eventually('owned native file dialog', read, dialog),
    cancel: async () => { action('press', 'Cancel'); await dismissed(); },
    choose,
    confirmOpen,
    async save(name) {
      // The collapsed AppKit save panel can hide the suffix. Verify the
      // actual path, including its extension, after confirming the save.
      await eventually('sanitize and prefill download filename', read, rows => rows.some(row =>
        row.role === 'AXTextField' && (row.value === name || (row.focused && row.value === path.parse(name).name))));
      try { action('press', 'Save'); }
      catch (error) {
        // AppKit can display the overwrite sheet without completing AXPress.
        await eventually('native Save opened the overwrite sheet', read, confirmation).catch(() => { throw error; });
      }
    },
    async replace(accept) {
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
