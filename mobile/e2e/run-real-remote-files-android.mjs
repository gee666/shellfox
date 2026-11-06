// Real Android UI/native I/O against actual Windows files and production approved RemoteHost.
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { existsSync, readFileSync, writeFileSync } from 'node:fs';
import { copyFile, readFile } from 'node:fs/promises';
import path from 'node:path';
import { setTimeout as sleep } from 'node:timers/promises';
import { androidFilesDriver } from './android-files-driver.mjs';
import { startRemoteHost } from './start-remote-host.mjs';
import { controlBounds } from '../../scripts/mobile-e2e-controls.mjs';

const d = androidFilesDriver('mobile-real-remote-android');
d.requireApks(); assert(existsSync(d.remoteApk), 'Build the explicit real-remote QA APK first.');
const host = await startRemoteHost();
const screenshots = [], proof = { coverage: host.ready.coverage, hostPlatform: host.ready.hostPlatform, root: host.ready.root };
const capture = name => { d.screenshot(name); screenshots.push(path.join(d.output, `${name}.png`)); };
async function poll(check, message, timeout = 45_000) { const until = Date.now() + timeout; while (Date.now() < until) { const result = await check(); if (result) return result; await sleep(150); } throw new Error(message); }
async function savedText() { return readFile(path.join(host.ready.root, 'code.ts'), 'utf8'); }
let failure;
try {
  const downloadedName = 'shellfox-real-remote-download.bin';
  await copyFile(path.join(host.ready.root, 'transfer.bin'), path.join(host.ready.root, downloadedName));
  // Only this test package's named exports may be cleaned, never normal-client Downloads.
  d.shell('content', 'delete', '--uri', 'content://media/external/downloads', '--where', `"owner_package_name='${d.app}' AND _display_name='${downloadedName}'"`);
  d.install(d.remoteApk); await d.launch('remote');
  await d.find('Remote state awaiting-approval');
  await poll(async () => (await host.status()).pending.some(peer => peer.id === host.ready.configuration.clientPeer.id), 'The real host did not receive the pending Android peer.');
  assert(!d.dump().some(node => d.matches(node, 'Open file code.ts')), 'Files must not be exposed before real host approval.');
  capture('real-host-approval-required'); await host.control('approve'); await d.find('Open file code.ts'); capture('real-windows-directory');
  proof.approvalRequired = true; proof.actualWindowsListing = true;
  console.log('PASS Android production RemoteClient requested approval and listed actual Windows disk files after host approval.');
  await d.tap('Open file code.ts'); await d.find('Code preview'); await d.tap('Show file path'); await d.find('File path'); capture('real-windows-canonical-path'); await d.tap('Close path');
  if (d.qaClipboardReady()) await d.tap('Copy file path');
  await d.tap('Edit source'); d.tapNode(await d.find('File source')); d.shell('input', 'keycombination', 'KEYCODE_CTRL_LEFT', 'KEYCODE_A'); d.shell('input', 'text', 'RemoteSavedFromAndroid');
  await d.find('RemoteSavedFromAndroid'); await d.tap('Save'); await poll(async () => await savedText() === 'RemoteSavedFromAndroid', 'Native edit was not written to the actual Windows file.'); capture('real-host-text-saved');
  proof.textSavedOnWindowsDisk = true; console.log('PASS Native text edit/save changed the real Windows file, verified outside Android.');
  await d.tap('Close file'); await d.tap('Open file README.md'); await d.find('Windows remote device'); capture('real-host-markdown-image'); await d.tap('Close file');
  await d.tap('Open file index.html'); await d.find('Windows sibling JavaScript loaded'); await d.tap('Run remote JavaScript'); await d.find('Remote script ran on Android'); capture('real-host-interactive-html'); await d.tap('Close file');
  proof.markdownImage = true; proof.htmlSiblingJavaScriptRan = true;
  console.log('PASS Actual Windows Markdown/image and HTML/CSS/JavaScript sibling assets rendered and ran on Android.');
  await d.tap('Open file image.png'); await d.tap('Edit image');
  await d.wait(nodes => nodes.find(node => node.text === '192 × 96'), 'actual host image decoded'); await d.tap('Rotate right');
  const canvas = controlBounds(await d.find('Image canvas')); assert(canvas);
  d.shell('input', 'swipe', String(Math.round(canvas.left + (canvas.right - canvas.left) * .2)), String(Math.round(canvas.top + (canvas.bottom - canvas.top) * .2)), String(Math.round(canvas.left + (canvas.right - canvas.left) * .8)), String(Math.round(canvas.top + (canvas.bottom - canvas.top) * .8)), '700');
  await d.tap('Crop'); await d.tap('Save image');
  const changed = await poll(async () => { const image = (await host.status()).files.find(file => file.name === 'image.png'); return image?.png && image.png.width < 96 && image.png.height < 192 ? image : false; }, 'Native image edit was not saved on Windows.');
  capture('real-host-image-cropped-saved'); proof.imageSavedOnWindowsDisk = changed;
  await d.tap('Close file');
  const uploadName = 'shellfox-real-remote-upload.bin', upload = path.join(d.output, uploadName), bytes = Buffer.alloc(192 * 1024 * 2 + 47);
  for (let index = 0; index < bytes.length; index++) bytes[index] = (index * 19 + 7) % 251;
  writeFileSync(upload, bytes); d.adb(['push', upload, `/sdcard/Download/${uploadName}`]);
  await d.tap('Upload to this folder'); await d.tap('Show roots'); d.tapNode(await d.wait(nodes => nodes.find(node => node.text === 'Downloads' && node.resource === 'android:id/title'), 'system Downloads location'));
  d.tapNode(await d.find(uploadName)); await poll(async () => { try { return (await readFile(path.join(host.ready.root, uploadName))).equals(bytes); } catch { return false; } }, 'Picked local file did not arrive byte-exact on the actual Windows disk.');
  await d.wait(nodes => nodes.find(node => node.text.includes('Uploaded ') && node.text.includes(uploadName)), 'native upload acknowledgement'); capture('real-picker-upload-to-windows');
  proof.uploadSha256 = createHash('sha256').update(await readFile(path.join(host.ready.root, uploadName))).digest('hex'); assert.equal(proof.uploadSha256, createHash('sha256').update(bytes).digest('hex'));
  console.log('PASS Real Android document picker uploaded multiple chunks to Windows with exact SHA256.');
  await d.reveal(`Open file ${downloadedName}`); await d.tap(`Open file ${downloadedName}`); await d.tap('Download selected file');
  await d.find(`Saved to Downloads: ${downloadedName}`); capture('real-windows-file-saved-to-downloads');
  const local = path.join(d.output, downloadedName); d.adb(['pull', `/sdcard/Download/${downloadedName}`, local], 'utf8', 120_000);
  const expected = host.ready.initialFiles.find(file => file.name === 'transfer.bin').sha256;
  proof.downloadSha256 = createHash('sha256').update(readFileSync(local)).digest('hex'); assert.equal(proof.downloadSha256, expected);
  proof.mediaStore = d.shell('content', 'query', '--uri', 'content://media/external/downloads', '--projection', '_display_name:is_pending:owner_package_name', '--where', `"_display_name='${downloadedName}' AND owner_package_name='${d.app}'"`);
  assert(proof.mediaStore.includes('is_pending=0')); console.log('PASS Actual Windows file downloaded to Android MediaStore Downloads with exact SHA256.');
  await host.control('revoke'); await d.wait(nodes => nodes.find(node => node.label === 'Download selected file' && !node.enabled), 'real revoked host unavailable'); capture('real-host-revoked-offline-file');
  const before = (await host.status()).requests.filter(request => request.method === 'listFiles').length;
  await d.tap('Close file'); await d.find('Open file code.ts'); capture('real-offline-origin-directory');
  assert.equal((await host.status()).requests.filter(request => request.method === 'listFiles').length, before, 'Offline file Close should restore a directory snapshot without another RPC.');
  proof.revokedOfflineCloseUsesSnapshot = true;
  proof.status = await host.status(); console.log('PASS Real host revocation disables operations; file Close restores its cached Windows directory without RPC.');
} catch (cause) {
  failure = cause; writeFileSync(path.join(d.output, 'failure-ui.json'), JSON.stringify(d.dump(), null, 2)); capture('failure');
} finally {
  await d.cleanupQa().catch(() => {});
  writeFileSync(path.join(d.output, 'real-device-proof.json'), JSON.stringify({ ...proof, screenshots, passed: !failure }, null, 2));
  await host.stop();
  if (process.env.SHELLFOX_FILES_KEEP_QA !== '1') await d.restoreDemo();
}
console.log(`Real Windows host artifacts: ${host.runDirectory}`); console.log(`Android artifacts: ${d.output}`);
if (failure) { console.error(failure); process.exitCode = 1; }
