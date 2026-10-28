#!/usr/bin/env node
// Tests the isolated terminalTest APK. Never installs over the normal app or changes its data.
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { mkdirSync, readdirSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { setTimeout as sleep } from 'node:timers/promises';
import { DOMParser } from '@xmldom/xmldom';
import { parseHierarchy } from './mobile-e2e-controls.mjs';
import { checkGboardInput } from './mobile-terminal-ime.mjs';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const args = process.argv.slice(2);
const option = name => args[args.indexOf(name) + 1];
if (args.includes('--help')) {
  console.log('node scripts/mobile-terminal-native-e2e.mjs --serial emulator-5554 --apk terminal-harness.apk [--gboard]');
  process.exit(0);
}
for (const key of ['--serial', '--apk']) assert(args.includes(key) && option(key) && !option(key).startsWith('--'), `Required ${key}`);
const serial = option('--serial');
assert(/^emulator-\d+$/.test(serial), 'This fixture is restricted to an explicitly named emulator.');
// Inspect the manifest before any adb call. A typo must never install over the real app.
const sdk = process.env.ANDROID_HOME ?? process.env.ANDROID_SDK_ROOT;
assert(sdk, 'Set ANDROID_HOME or ANDROID_SDK_ROOT to validate the fixture APK.');
const tools = readdirSync(path.join(sdk, 'build-tools')).filter(name => /^\d+\.\d+\.\d+$/.test(name))
  .sort((a, b) => b.localeCompare(a, 'en', { numeric: true }));
assert(tools[0], 'Android SDK build-tools are required.');
const aapt = path.join(sdk, 'build-tools', tools[0], process.platform === 'win32' ? 'aapt.exe' : 'aapt');
const manifest = execFileSync(aapt, ['dump', 'badging', path.resolve(option('--apk'))], { encoding: 'utf8', timeout: 30000, windowsHide: true });
assert.match(manifest, /^package: name='com\.shellfox\.mobile\.terminaltest' /m, 'Refusing to install an APK outside the isolated terminal test package.');
const output = path.join(root, 'tmp/mobile-terminal-native-e2e', new Date().toISOString().replace(/[:.]/g, '-'));
mkdirSync(output, { recursive: true });
function adb(parts, binary = false, timeout = 20000) {
  return execFileSync(process.env.ADB ?? 'adb', ['-s', serial, ...parts], {
    timeout, encoding: binary ? 'buffer' : 'utf8', windowsHide: true, maxBuffer: 8 * 1024 * 1024,
  });
}
const shell = (...parts) => adb(['shell', ...parts]);
const app = 'com.shellfox.mobile.terminaltest';
const activity = `${app}/com.shellfox.mobile.MainActivity`;
function dump() {
  const raw = adb(['exec-out', 'uiautomator', 'dump', '/dev/tty']);
  const xml = parseHierarchy(raw);
  if (xml === undefined) {
    // Keep the last unavailable-root response for timeout diagnostics, but let
    // waitFor retain its existing deadline while Android settles the UI tree.
    const safe = raw.slice(0, 16384).replace(/https?:\/\/[^\s"<>]+/g, '[redacted-url]')
      .replace(/tskey-[a-zA-Z0-9_-]+/g, '[redacted-key]');
    writeFileSync(path.join(output, 'uiautomator-unavailable.txt'), safe);
    return [];
  }
  const dom = new DOMParser({ onError: () => {} }).parseFromString(xml, 'text/xml');
  return Array.from(dom.getElementsByTagName('node')).map(node => ({
    text: node.getAttribute('text') ?? '', label: node.getAttribute('content-desc') ?? '', bounds: node.getAttribute('bounds') ?? '', type: node.getAttribute('class') ?? '', resource: node.getAttribute('resource-id') ?? '',
  }));
}
async function waitFor(check, description) {
  const until = Date.now() + 30000;
  while (Date.now() < until) { const value = check(dump()); if (value) return value; await sleep(400); }
  throw new Error(`Timed out waiting for ${description}`);
}
const control = (tree, label) => tree.find(node => node.label === label);
function tapNode(node) {
  const bounds = node.bounds.match(/\d+/g)?.map(Number);
  assert(bounds?.length === 4, 'Control has no bounds');
  shell('input', 'tap', `${Math.round((bounds[0] + bounds[2]) / 2)}`, `${Math.round((bounds[1] + bounds[3]) / 2)}`);
}
async function tap(label) { tapNode(await waitFor(tree => control(tree, label), label)); }
async function fillDirectory(value) {
  assert(/^[~A-Za-z0-9_./-]+$/.test(value), 'Unsafe fixture path');
  await tap('Host directory');
  shell('input', 'keycombination', 'KEYCODE_CTRL_LEFT', 'KEYCODE_A');
  shell('input', 'text', `'${value}'`);
  await waitFor(tree => control(tree, 'Host directory')?.text === value, 'typed directory draft');
}
function delivered(tree) {
  const value = tree.find(node => /^Delivery \d+\/\d+$/.test(node.text))?.text.match(/(\d+)\/(\d+)/);
  return value && Number(value[1]) > 1 && value[1] === value[2];
}
const screenshot = name => writeFileSync(path.join(output, `${name}.png`), adb(['exec-out', 'screencap', '-p'], true));
function status(tree) { return tree.find(node => /^Rendered \d+; grid \d+x\d+; input /.test(node.text))?.text; }
function rows(value) { return Number(value.match(/grid \d+x(\d+)/)?.[1]); }
function imeTop() {
  const windows = shell('dumpsys', 'window', 'windows');
  const ime = windows.match(/Window #\d+ Window\{[^}]* InputMethod\}:([\s\S]*?)(?=\n  Window #|$)/)?.[1];
  if (!ime?.includes('isVisible=true')) return null;
  return Number(ime.match(/touchable region=SkRegion\(\(\d+,(\d+),/)?.[1]);
}
let installed = false, failure;
const since = shell('date', "'+%m-%d %H:%M:%S.000'").trim();
const originalKeyboardSetting = shell('settings', 'get', 'secure', 'show_ime_with_hard_keyboard').trim();
try {
  shell('settings', 'put', 'secure', 'show_ime_with_hard_keyboard', '1');
  assert.equal(shell('getprop', 'sys.boot_completed').trim(), '1');
  adb(['install', '-r', path.resolve(option('--apk'))], false, 180000); installed = true;
  shell('am', 'force-stop', app); shell('am', 'start', '-W', '-n', activity);
  await tap('Toggle terminal fixture');
  const initial = await waitFor(tree => status(tree), 'native WebView replay and resize');
  await waitFor(tree => control(tree, 'Ctrl'), 'key row with keyboard hidden');
  screenshot('terminal-keyboard-hidden');
  await tap('Keyboard');
  await waitFor(tree => {
    const top = imeTop(), ctrl = control(tree, 'Ctrl');
    const bottom = Number(ctrl?.bounds.match(/\d+/g)?.[3]);
    return top && bottom > 0 && bottom <= top && rows(status(tree) ?? '') < rows(initial);
  }, 'first explicit Keyboard action opens IME and refits terminal');
  screenshot('terminal-keyboard-first-open');
  await tap('Keyboard');
  await waitFor(() => !imeTop(), 'Keyboard hides IME before touch test');
  const display = await waitFor(tree => tree.find(node => node.type === 'android.webkit.WebView'), 'terminal display');
  const displayBounds = display.bounds.match(/\d+/g).map(Number);
  shell('input', 'tap', `${Math.round((displayBounds[0] + displayBounds[2]) / 2)}`, `${displayBounds[1] + 80}`);
  await waitFor(tree => {
    const top = imeTop(), ctrl = control(tree, 'Ctrl');
    const bottom = Number(ctrl?.bounds.match(/\d+/g)?.[3]);
    return top && bottom > 0 && bottom <= top && rows(status(tree) ?? '') < rows(initial);
  }, 'key row above IME and smaller terminal grid');
  shell('input', 'text', 'terminal_native_ok');
  await waitFor(tree => /input terminal_native_ok$/.test(status(tree) ?? '') && delivered(tree), 'native input echo rendered by xterm');
  screenshot('terminal-keyboard-open');
  await tap('Keyboard');
  await waitFor(tree => !imeTop() && control(tree, 'Ctrl') && rows(status(tree) ?? '') >= rows(initial), 'restored grid after keyboard dismissal');
  await tap('Keyboard');
  await waitFor(tree => {
    const top = imeTop(), ctrl = control(tree, 'Ctrl');
    const bottom = Number(ctrl?.bounds.match(/\d+/g)?.[3]);
    return top && bottom > 0 && bottom <= top;
  }, 'explicit Keyboard button opens IME above the key row');
  screenshot('terminal-keyboard-button');
  await tap('Special keys');
  await waitFor(tree => !imeTop() && control(tree, 'Alt') && control(tree, 'F1'), 'special keys replace system keyboard');
  await tap('F1'); await tap('Ctrl'); await tap('c'); await tap('c'); await tap('Ctrl');
  await waitFor(tree => (status(tree) ?? '').endsWith('terminal_native_ok\\x1bOP\\x03\\x03') && delivered(tree), 'F1 and exactly two Ctrl+C bytes rendered');
  screenshot('terminal-special-keys');
  await tap('Keyboard');
  await waitFor(tree => imeTop() && !control(tree, 'Alt'), 'system keyboard replaces special keys');
  await tap('Special keys');
  await waitFor(tree => !imeTop() && control(tree, 'Alt'), 'special keys replace system keyboard again');
  await tap('Special keys');
  await waitFor(tree => !imeTop() && !control(tree, 'Alt'), 'special keys toggle closed without opening IME');
  await tap('Toggle terminal fixture'); await tap('Toggle terminal fixture');
  await waitFor(tree => status(tree) && control(tree, 'Ctrl'), 'terminal remount');
  const attachment = path.join(output, 'shellfox-attachment.txt');
  writeFileSync(attachment, 'Shellfox attachment fixture 0123456789\n'.repeat(6000));
  adb(['push', attachment, '/sdcard/Download/shellfox-attachment.txt']);
  await tap('Attach');
  await waitFor(tree => control(tree, 'Show roots'), 'system document picker');
  screenshot('attachment-picker');
  await tap('Show roots');
  tapNode(await waitFor(tree => tree.find(node => node.text === 'Downloads' && node.resource === 'android:id/title'), 'Downloads navigation item'));
  tapNode(await waitFor(tree => tree.find(node => node.text === 'shellfox-attachment.txt'), 'test-owned document'));
  await waitFor(tree => tree.some(node => /^Uploaded shellfox-attachment\.txt; bytes \d+; chunks 2; exact$/.test(node.text)), 'exact multi-chunk native attachment upload');
  await waitFor(tree => control(tree, 'Copy path') && control(tree, 'Insert path'), 'uploaded path actions');
  screenshot('attachment-uploaded');
  await tap('Insert path');
  await waitFor(tree => (status(tree) ?? '').endsWith('/home/mobile/uploads/mobile attachment.txt') && delivered(tree), 'inserted path without a submitted command');
  await tap('Dismiss attachment');
  await tap('Attach');
  await waitFor(tree => control(tree, 'Show roots'), 'second system picker');
  shell('input', 'keyevent', 'KEYCODE_BACK');
  await waitFor(tree => control(tree, 'Attach') && !control(tree, 'Cancel attachment'), 'native picker cancellation returns to terminal');
  screenshot('attachment-cancelled');
  if (args.includes('--gboard')) await checkGboardInput({ adb, shell, tap, waitFor, output });
  await tap('Directory fixture');
  await waitFor(tree => control(tree, 'Host directory'), 'native directory modal');
  await fillDirectory('~/wo');
  await waitFor(tree => {
    const choice = control(tree, '~/work/'), top = imeTop();
    return choice && control(tree, '~/workshop/') && top && Number(choice.bounds.match(/\d+/g)?.[3]) <= top;
  }, 'filtered directory choices above the system keyboard');
  screenshot('directory-suggestions');
  await tap('~/work/');
  await waitFor(tree => control(tree, 'Host directory')?.text === '~/work/', 'directory choice does not submit');
  shell('input', 'text', 's');
  await waitFor(tree => control(tree, 'Host directory')?.text === '~/work/s', 'completion leaves caret at path end');
  shell('input', 'keyevent', 'KEYCODE_DEL');
  await tap('OK');
  await waitFor(tree => control(tree, 'Created session /home/mobile/work; sessions 1; folders 0'), 'existing directory creates one session');
  await tap('Open directory fixture'); await fillDirectory('~/new/child'); await tap('OK');
  await waitFor(tree => control(tree, 'No') && control(tree, 'Yes'), 'recursive creation confirmation');
  await tap('No');
  await waitFor(tree => control(tree, 'Host directory')?.text === '~/new/child', 'No preserves original path draft');
  await tap('OK'); await tap('Yes');
  await waitFor(tree => control(tree, 'Created session /home/mobile/new/child; sessions 2; folders 1'), 'recursive creation creates only one session');
  await tap('Open directory fixture'); await fillDirectory('~/data');
  await waitFor(tree => !control(tree, '~/data/') && control(tree, 'Host directory')?.text === '~/data', 'sole exact path hides suggestions');
  await tap('Cancel'); screenshot('directory-finished');
  shell('pidof', app);
} catch (error) {
  failure = error;
  try { screenshot('failure'); writeFileSync(path.join(output, 'failure-ui.json'), JSON.stringify(dump(), null, 2)); }
  catch { /* Crash logs still capture failures when the app is gone. */ }
}
finally {
  try {
    const logs = adb(['logcat', '-d', '-T', since, 'AndroidRuntime:E', 'ReactNativeJS:E', 'libc:F', '*:S']);
    writeFileSync(path.join(output, 'errors.log'), logs);
    if (/FATAL EXCEPTION|Fatal signal|ReactNativeJS:.*(?:Error|Exception)/.test(logs)) failure ??= new Error('Native crash or JS error. See errors.log.');
  } catch (error) { failure ??= error; }
  try { shell('rm', '-f', '/sdcard/Download/shellfox-attachment.txt'); } catch (error) { failure ??= error; }
  if (installed) {
    try { shell('am', 'force-stop', app); }
    catch (error) { failure ??= error; }
  }
  try {
    if (originalKeyboardSetting === 'null') shell('settings', 'delete', 'secure', 'show_ime_with_hard_keyboard');
    else shell('settings', 'put', 'secure', 'show_ime_with_hard_keyboard', originalKeyboardSetting);
  } catch (error) { failure ??= error; }
}
console.log(`Artifacts: ${path.relative(root, output)}`);
if (failure) { console.error(failure.message); process.exitCode = 1; }
else console.log('PASS native terminal replay/input, system and special keyboards, Ctrl+C bytes, attachment picker/upload/insert/cancel, IME layout, and directory suggestions/creation. Isolated test app stopped. No authentication changed.');
