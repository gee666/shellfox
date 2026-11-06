// Isolated native caret gate before building the demo. No Tab or pointer re-position after mode switch.
import assert from 'node:assert/strict';
import { writeFileSync } from 'node:fs';
import path from 'node:path';
import { androidFilesDriver } from './android-files-driver.mjs';
const d = androidFilesDriver('mobile-caret-native'); d.requireApks();
let failure;
try {
  d.install(d.qaApk); await d.launch('qa'); await d.tap('Open files fixture'); await d.tap('Open file code.ts');
  if (d.qaClipboardReady()) await d.tap('Copy file path');
  await d.tap('Edit source'); d.tapNode(await d.find('File source'));
  d.shell('input', 'keycombination', 'KEYCODE_CTRL_LEFT', 'KEYCODE_A'); d.shell('input', 'text', 'NativeSaved'); await d.find('NativeSaved');
  d.shell('input', 'keyevent', 'KEYCODE_MOVE_HOME'); d.shell('input', 'keyevent', 'KEYCODE_DPAD_RIGHT'); d.shell('input', 'keyevent', 'KEYCODE_DPAD_RIGHT');
  d.shell('input', 'text', 'Q'); await d.find('NaQtiveSaved'); // Prove the ADB keys really reached index two.
  d.shell('input', 'keyevent', 'KEYCODE_DEL'); await d.find('NativeSaved');
  await d.tap('Read file'); await d.find('Code preview'); await d.tap('Edit source'); await d.find('File source');
  assert(d.dump().find(node => node.label === 'File source')?.focused, 'Explicit Pencil must focus the native editor.');
  d.shell('input', 'text', 'X'); await d.find('NaXtiveSaved'); await d.tap('Save');
  await d.wait(nodes => nodes.find(node => node.label === 'Save' && !node.enabled), 'acknowledged saved caret edit'); d.screenshot('caret-saved');
  await d.tap('Close file'); await d.leaveToQa(); await d.find('Saved text: NaXtiveSaved');
  writeFileSync(path.join(d.output, 'proof.json'), JSON.stringify({ passed: true, indexBeforeToggle: 2, textAfterReadEditInsertion: 'NaXtiveSaved', savedTextVerified: true }, null, 2));
  console.log('PASS Native focus, index-two caret across Read/Edit, insertion without Tab/pointer/Ctrl+A, and saved exact text.');
} catch (cause) { failure = cause; writeFileSync(path.join(d.output, 'failure-ui.json'), JSON.stringify(d.dump(), null, 2)); d.screenshot('failure'); }
finally { await d.cleanupQa().catch(() => {}); }
console.log(`Artifacts: ${d.output}`);
if (failure) { console.error(failure); process.exitCode = 1; }
