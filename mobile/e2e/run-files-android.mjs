// Native UI/WebView integration for the isolated FilesHarness APK only.
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { mkdirSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { setTimeout as sleep } from 'node:timers/promises';
import { DOMParser } from '@xmldom/xmldom';
import { controlBounds, parseHierarchy } from '../../scripts/mobile-e2e-controls.mjs';

const root = fileURLToPath(new URL('../../', import.meta.url));
const sdk = process.env.ANDROID_HOME;
assert(sdk, 'ANDROID_HOME is required.');
const serial = process.env.ANDROID_SERIAL ?? 'emulator-5554';
assert(/^emulator-\d+$/.test(serial), 'Only an emulator may run the Files fixture.');
const adbPath = path.join(sdk, 'platform-tools', process.platform === 'win32' ? 'adb.exe' : 'adb');
const apk = path.resolve(process.argv[2] ?? path.join(root, 'mobile/android/app/build/outputs/apk/terminalTest/app-terminalTest.apk'));
const aapt = path.join(sdk, 'build-tools/36.0.0', process.platform === 'win32' ? 'aapt.exe' : 'aapt');
const manifest = execFileSync(aapt, ['dump', 'badging', apk], { encoding: 'utf8', timeout: 30_000 });
assert.match(manifest, /^package: name='com\.shellfox\.mobile\.terminaltest' /m, 'Refusing to replace the production mobile app.');
const app = 'com.shellfox.mobile.terminaltest';
const output = path.join(root, 'tmp/mobile-files-native', new Date().toISOString().replace(/[:.]/g, '-'));
mkdirSync(output, { recursive: true });
function adb(args, encoding = 'utf8', timeout = 30_000) {
  return execFileSync(adbPath, ['-s', serial, ...args], { encoding, timeout, windowsHide: true, maxBuffer: 16 * 1024 * 1024 });
}
const shell = (...args) => adb(['shell', ...args]);
function dump() {
  const xml = parseHierarchy(adb(['exec-out', 'uiautomator', 'dump', '/dev/tty']));
  if (!xml) return [];
  return Array.from(new DOMParser().parseFromString(xml, 'text/xml').getElementsByTagName('node')).map(node => ({
    text: node.getAttribute('text') ?? '', label: node.getAttribute('content-desc') ?? '',
    bounds: node.getAttribute('bounds') ?? '', type: node.getAttribute('class') ?? '', enabled: node.getAttribute('enabled') !== 'false',
  }));
}
function matches(node, text) { return node.label.toLowerCase() === text.toLowerCase() || node.text.toLowerCase() === text.toLowerCase(); }
async function find(text, enabled = false) {
  const until = Date.now() + 45_000;
  while (Date.now() < until) {
    const nodes = dump();
    const node = nodes.find(item => item.label.toLowerCase() === text.toLowerCase()) ?? nodes.find(item => item.text.toLowerCase() === text.toLowerCase());
    if (node && (!enabled || node.enabled)) return node;
    await sleep(400);
  }
  throw new Error(`Missing native control: ${text}`);
}
function tapNode(node) {
  const bounds = controlBounds(node); assert(bounds, 'Control has no visible bounds.');
  shell('input', 'tap', String(Math.round((bounds.left + bounds.right) / 2)), String(Math.round((bounds.top + bounds.bottom) / 2)));
}
async function tap(text) { tapNode(await find(text, true)); await sleep(250); }
async function imageStatus(dirty) {
  const until = Date.now() + 45000;
  while (Date.now() < until) {
    const status = dump().find(node => /^\d+ × \d+(?: · Unsaved)?$/.test(node.text) && node.text.includes('Unsaved') === dirty);
    if (status) return status.text;
    await sleep(250);
  }
  throw new Error('Image canvas state did not settle.');
}
async function screenshot(name) { writeFileSync(path.join(output, `${name}.png`), adb(['exec-out', 'screencap', '-p'], 'buffer')); }
async function enter(text) {
  assert(/^[A-Za-z]+$/.test(text));
  tapNode(await find('File source'));
  shell('input', 'keycombination', 'KEYCODE_CTRL_LEFT', 'KEYCODE_A');
  shell('input', 'text', text);
  await find(text);
}
let failure;
try {
  assert.equal(shell('getprop', 'sys.boot_completed').trim(), '1');
  adb(['install', '-r', apk], 'utf8', 180_000);
  shell('am', 'force-stop', app);
  shell('am', 'start', '-W', '-n', `${app}/com.shellfox.mobile.MainActivity`);
  await tap('Open files fixture'); await find('Open file code.ts');
  await screenshot('files-directory');
  if (process.env.SHELLFOX_FILES_IMAGE_ONLY !== '1') {
  await tap('Open folder subdir'); await find('Open file nested.txt');
  assert(!dump().some(node => matches(node, 'Open file code.ts')), 'Folder navigation must not show the parent listing.');
  await tap('Up'); await find('Open file code.ts');
  console.log('PASS Native folder navigation and session/tab-scoped fixture requests.');
  await tap('Open file code.ts'); await find('Code preview'); await screenshot('code-highlighted');
  await tap('Copy file path'); await find('Path copied.'); await tap('‹ Terminal'); await tap('Read clipboard fixture'); await find('Clipboard: /fixture/code.ts');
  console.log('PASS Canonical remote file path copied to the local Android clipboard.');
  await tap('Open files fixture'); await tap('Open file code.ts');
  await tap('Source'); await enter('NativeSaved');
  assert.equal(dump().find(node => node.label === 'File path' && node.type === 'android.widget.EditText')?.text, '/fixture/code.ts');
  // Switching view closes Gboard's floating hardware-keyboard toolbar, which may cover Save.
  await tap('Code'); await tap('Save'); assert.equal((await find('Save')).enabled, false); await screenshot('native-source-saved');
  await tap('‹ Terminal'); await find('Saved text: NativeSaved');
  console.log('PASS Native code preview, Source edit, revision save and modal exit.');
  await tap('Open files fixture'); await tap('Open file code.ts'); await tap('Source');
  await enter('UnsavedNative'); await tap('‹ Files'); await find('Discard changes?');
  await tap('Keep editing'); await find('UnsavedNative');
  await tap('‹ Files'); await tap('Discard'); await find('Open file README.md');
  console.log('PASS Native dirty confirmation keeps or discards source edits.');
  await tap('Open file README.md'); await find('Native Markdown'); await find('Markdown sibling loaded');
  await screenshot('markdown-sibling'); await tap('‹ Terminal'); await find('Loaded asset: image.png');
  console.log('PASS Android WebView rendered Markdown and requested its scoped sibling image.');
  await tap('Open files fixture'); await tap('Open file index.html'); await tap('Run preview'); await find('123');
  await screenshot('interactive-html'); await tap('‹ Files');
  console.log('PASS Android opaque HTML preview runs inline handlers and sibling JavaScript.');
  await tap('Open file icon.svg'); await find('SVG preview'); await tap('Source'); await find('File source');
  await tap('‹ Files');
  console.log('PASS Native SVG preview and editable source.');
  await tap('Open file icon.ico'); await find('Image preview'); await screenshot('ico-preview'); await tap('‹ Files');
  console.log('PASS Native ICO uses isolated static WebView preview.');
  }
  await tap('Open file image.png'); await tap('Edit image'); await tap('Rotate right');
  const rotated = await imageStatus(true); await screenshot('image-rotated');
  const canvas = controlBounds(await find('Image canvas')); assert(canvas, 'Image canvas must be visible for touch cropping.');
  shell('input', 'swipe', String(Math.round(canvas.left + (canvas.right - canvas.left) * .2)), String(Math.round(canvas.top + (canvas.bottom - canvas.top) * .2)),
    String(Math.round(canvas.left + (canvas.right - canvas.left) * .8)), String(Math.round(canvas.top + (canvas.bottom - canvas.top) * .8)), '700');
  await screenshot('image-crop-selection'); await tap('Crop');
  assert.notEqual(await imageStatus(true), rotated, 'Touch cropping must change the canvas dimensions.');
  await screenshot('image-cropped'); await tap('Save image'); await imageStatus(false); await screenshot('image-saved');
  await tap('‹ Terminal'); await find('Saved image');
  console.log('PASS Android canvas rotate, touch-drag crop and authenticated WebView Save image.');
  await tap('Open files fixture'); await find('Open file code.ts'); shell('input', 'keyevent', 'KEYCODE_BACK');
  await find('Open files fixture'); await screenshot('hardware-back');
  console.log('PASS Android hardware back closes Files without leaving the fixture.');
} catch (cause) {
  failure = cause;
  writeFileSync(path.join(output, 'failure-ui.json'), JSON.stringify(dump(), null, 2));
  await screenshot('failure');
} finally {
  const pid = shell('pidof', app).trim();
  if (pid) {
    const logs = adb(['logcat', '-d', `--pid=${pid}`, '-v', 'brief', 'AndroidRuntime:E', 'ReactNativeJS:E', 'chromium:E', '*:S']);
    writeFileSync(path.join(output, 'app.log'), logs);
    if (/FATAL EXCEPTION|(?:^|\n)E\/ReactNativeJS/.test(logs)) failure ??= new Error('Android or React Native runtime error. Inspect app.log.');
  }
}
console.log(`Artifacts: ${path.relative(root, output)}`);
if (failure) { console.error(failure); process.exitCode = 1; }
