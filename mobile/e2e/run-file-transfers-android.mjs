// Real native document picker + scoped MediaStore Downloads. Only remote API bytes are fixtures.
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { readFileSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { setTimeout as sleep } from 'node:timers/promises';
import { androidFilesDriver } from './android-files-driver.mjs';

const d = androidFilesDriver();
d.requireApks();
const chunk = 192 * 1024, downloadSize = chunk * 2 + 19, uploadSize = chunk + 37, largeSize = 64 * 1024 * 1024 + 17;
const prefix = '/sdcard/Download/shellfox-transfer-';
function pattern(size, offset = 0) { return Buffer.from(Uint8Array.from({ length: size }, (_, index) => (offset + index) % 251)); }
function expectedHash(size) { const hash = createHash('sha256'); for (let offset = 0; offset < size; offset += chunk) hash.update(pattern(Math.min(chunk, size - offset), offset)); return hash.digest('hex'); }
function verifyDownload(name, size) {
  const local = path.join(d.output, name);
  d.adb(['pull', `${prefix}${name}`, local], 'utf8', 120_000);
  const bytes = readFileSync(local);
  assert.equal(bytes.length, size); assert.equal(createHash('sha256').update(bytes).digest('hex'), expectedHash(size));
  return local;
}
async function chooseUpload(name) {
  await d.tap('Upload to this folder'); await d.find('Show roots'); d.screenshot('system-upload-picker');
  await d.tap('Show roots');
  d.tapNode(await d.wait(nodes => nodes.find(node => node.text === 'Downloads' && node.resource === 'android:id/title'), 'system Downloads location'));
  d.tapNode(await d.find(name)); await sleep(400); d.screenshot('upload-progress');
}
const mediaRows = () => d.shell('content', 'query', '--uri', 'content://media/external/downloads', '--projection', '_id:_display_name:relative_path:is_pending', '--where', '"_display_name LIKE \'shellfox-transfer-%\'"');
let failure;
try {
  assert.equal(d.shell('getprop', 'sys.boot_completed').trim(), '1');
  d.install(d.qaApk);
  d.shell('content', 'delete', '--uri', 'content://media/external/downloads', '--where', `"owner_package_name='${d.app}' AND _display_name LIKE 'shellfox-transfer-%'"`);
  const remaining = d.shell('ls', '-a', '/sdcard/Download').split(/\r?\n/);
  for (const name of ['download.bin', 'empty.bin', 'large.bin', 'abandoned.bin']) assert(!remaining.includes(`shellfox-transfer-${name}`), `Refusing to overwrite an existing Downloads file not removed through test-owned MediaStore rows: ${name}`);
  const upload = path.join(d.output, 'shellfox-transfer-upload.bin'), empty = path.join(d.output, 'shellfox-transfer-upload-empty.bin');
  writeFileSync(upload, pattern(uploadSize)); writeFileSync(empty, Buffer.alloc(0));
  d.adb(['push', upload, `${prefix}upload.bin`]); d.adb(['push', empty, `${prefix}upload-empty.bin`]);
  await d.launch(); await d.tap('Open host files fixture'); await d.find('Open file code.ts'); d.screenshot('host-files-no-session');
  await d.tap('Open file shellfox-transfer-download.bin'); await d.tap('Download selected file'); d.screenshot('download-progress');
  await d.find('Saved to Downloads: shellfox-transfer-download.bin'); d.screenshot('download-saved');
  verifyDownload('download.bin', downloadSize);
  const published = mediaRows(); writeFileSync(path.join(d.output, 'mediastore-published.txt'), published);
  assert(published.split('\n').some(line => line.includes('shellfox-transfer-download.bin') && line.includes('is_pending=0')), 'Downloaded file must be published in MediaStore.');
  console.log('PASS Real MediaStore Downloads file exact bytes/hash, multichunk streaming and host-only access.');
  await d.tap('Close file'); await chooseUpload('shellfox-transfer-upload.bin');
  await d.find('Uploaded /fixture/shellfox-transfer-upload.bin'); d.screenshot('upload-saved');
  await d.leaveToQa(); await d.find(`Uploaded fixture; bytes ${uploadSize}; chunks 2; exact`);
  console.log('PASS Real system document picker and native chunk reader upload exact bytes without a terminal.');
  await d.tap('Open host files fixture'); await chooseUpload('shellfox-transfer-upload-empty.bin');
  await d.find('Uploaded /fixture/shellfox-transfer-upload-empty.bin'); await d.leaveToQa();
  await d.find('Uploaded fixture; bytes 0; chunks 0; exact');
  await d.tap('Open host files fixture'); await d.tap('Open file shellfox-transfer-empty.bin'); await d.tap('Download selected file');
  await d.find('Saved to Downloads: shellfox-transfer-empty.bin'); verifyDownload('empty.bin', 0);
  console.log('PASS Empty native uploads/downloads begin and finish with no data writes.');
  await d.tap('Close file'); await chooseUpload('shellfox-transfer-upload.bin');
  await d.find('The selected remote file already exists.');
  console.log('PASS Upload create rejects an existing remote name without overwriting.');
  await d.tap('Open file shellfox-transfer-cancel.bin'); await d.tap('Download selected file'); await d.tap('Cancel transfer');
  await d.find('Transfer cancelled.');
  assert(!d.shell('ls', '-a', '/sdcard/Download').split('\n').some(name => name.includes('pending') && name.includes('shellfox-transfer-cancel')), 'Cancelled pending Downloads row must be removed.');
  console.log('PASS Native pending download cancellation removes its partial file.');
  await d.tap('Close file'); await d.tap('Open file shellfox-transfer-large.bin');
  await d.find(`Fixture preview is too large: ${largeSize} bytes.`); await d.tap('Download selected file'); d.screenshot('large-download-progress');
  await d.find('Saved to Downloads: shellfox-transfer-large.bin'); verifyDownload('large.bin', largeSize);
  console.log('PASS File larger than preview cap remains selectable and downloads with exact bytes/hash.');
  await d.leaveToQa(); await d.tap('Stage pending download fixture'); await d.find('Staged pending download');
  assert(d.shell('ls', '-a', '/sdcard/Download').includes('shellfox-transfer-abandoned'), 'Pending file should exist before simulated process death.');
  await d.launch();
  await d.wait(() => !d.shell('ls', '-a', '/sdcard/Download').includes('shellfox-transfer-abandoned'), 'restart cleanup of only abandoned pending downloads');
  verifyDownload('download.bin', downloadSize);
  console.log('PASS Process restart cleans an abandoned pending MediaStore file and preserves a published download.');
  await d.tap('Open host files fixture');
  await d.tap('Open file code.ts');
  if (d.qaClipboardReady()) await d.tap('Copy file path'); else console.log('SKIP Native clipboard copy: unknown/non-text clipboard left untouched.');
  await d.tap('Close file');
  await d.tap('Change directory');
  d.tapNode(await d.wait(nodes => nodes.find(node => node.type === 'android.widget.EditText' && node.label === 'File path'), 'editable File path')); d.shell('input', 'keycombination', 'KEYCODE_CTRL_LEFT', 'KEYCODE_A'); d.shell('input', 'text', '/fixture/s');
  await d.find('/fixture/subdir/'); d.shell('input', 'keyevent', 'KEYCODE_ESCAPE'); await sleep(500);
  assert(!d.dump().some(node => d.matches(node, '/fixture/subdir/')), 'First hardware Escape must dismiss directory suggestions.');
  await d.find('File path'); d.shell('input', 'keyevent', 'KEYCODE_ESCAPE'); await d.find('Open file code.ts');
  d.shell('input', 'keyevent', 'KEYCODE_ESCAPE'); await d.find('Files QA launcher');
  console.log('PASS Native Escape dismisses suggestions, then the path sheet, then the Files root.');
} catch (cause) {
  failure = cause;
  writeFileSync(path.join(d.output, 'failure-ui.json'), JSON.stringify(d.dump(), null, 2)); d.screenshot('failure');
} finally {
  const pid = d.shell('pidof', d.app).trim();
  if (pid) {
    const logs = d.adb(['logcat', '-d', `--pid=${pid}`, '-v', 'brief', 'AndroidRuntime:E', 'ReactNativeJS:E', 'chromium:E', '*:S']);
    writeFileSync(path.join(d.output, 'app.log'), logs);
    if (/FATAL EXCEPTION|(?:^|\n)E\/ReactNativeJS/.test(logs)) failure ??= new Error('Native runtime error. Inspect app.log.');
  }
  try { if (process.env.SHELLFOX_FILES_KEEP_QA === '1') { await d.cleanupQa(); console.log('Explicit QA retained for visual review; restore demo after signoff.'); } else { await d.restoreDemo(); d.assertNoQaTools(); } } catch (cause) { failure ??= cause; }
}
console.log(`Artifacts: ${path.relative(d.root, d.output)}`);
if (failure) { console.error(failure); process.exitCode = 1; }
