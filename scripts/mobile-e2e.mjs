#!/usr/bin/env node
// Real Android UI smoke test. Requires a booted device and an installed APK or --apk.
// This does not enroll a Tailscale identity, use host keys, or simulate a tailnet.
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { mkdirSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { setTimeout as sleep } from 'node:timers/promises';
import { DOMParser } from '@xmldom/xmldom';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const options = process.argv.slice(2);
const option = name => { const i = options.indexOf(name); return i < 0 ? undefined : options[i + 1]; };
if (options.includes('--help')) {
  console.log('node scripts/mobile-e2e.mjs [--serial emulator-5554] [--apk path.apk] [--browser]');
  console.log('Tests signed-out UI and preference persistence. --browser opens real Tailscale sign-in but stops before authentication.');
  process.exit(0);
}
const serial = option('--serial') ?? process.env.ANDROID_SERIAL;
const adbPath = process.env.ADB ?? 'adb';
const app = 'com.shellfox.mobile';
const output = path.join(root, 'tmp', 'mobile-e2e', new Date().toISOString().replace(/[:.]/g, '-'));
mkdirSync(output, { recursive: true });
function adb(args, timeout = 20000, encoding = 'utf8') {
  try { return execFileSync(adbPath, [...(serial ? ['-s', serial] : []), ...args], { timeout, encoding, windowsHide: true, maxBuffer: 8 * 1024 * 1024, stdio: ['ignore', 'pipe', 'pipe'] }); }
  catch (cause) { throw new Error(`adb ${args[0]} ${args[1] ?? ''} failed: ${redact(cause instanceof Error ? cause.message.slice(0, 500) : 'device unavailable')}`); }
}
const shell = (...args) => adb(['shell', ...args]);
function nodes(xml) {
  const document = new DOMParser({ onError: () => {} }).parseFromString(xml, 'text/xml');
  return Array.from(document.getElementsByTagName('node')).map(node => ({
    text: node.getAttribute('text') ?? '', label: node.getAttribute('content-desc') ?? '',
    resource: node.getAttribute('resource-id') ?? '', type: node.getAttribute('class') ?? '',
    enabled: node.getAttribute('enabled') !== 'false', selected: node.getAttribute('selected') === 'true', bounds: node.getAttribute('bounds') ?? '',
  }));
}
function dump() {
  const raw = adb(['exec-out', 'uiautomator', 'dump', '/dev/tty'], 20000);
  const start = raw.indexOf('<?xml'); const end = raw.lastIndexOf('</hierarchy>');
  if (start < 0 || end < 0) return [];
  return nodes(raw.slice(start, end + '</hierarchy>'.length));
}
function matches(node, text) { return node.text === text || node.label === text; }
async function waitFor(predicate, description, timeout = 45000) {
  const until = Date.now() + timeout;
  while (Date.now() < until) {
    const tree = dump(); const result = predicate(tree);
    if (result) return result;
    await sleep(500);
  }
  throw new Error(`Timed out waiting for ${description}.`);
}
function control(tree, text) { return tree.find(node => node.label === text) ?? tree.find(node => node.text === text); }
async function find(text, timeout) { return waitFor(tree => control(tree, text), text, timeout); }
function tapNode(node) {
  assert(node.enabled, 'The requested UI control is disabled.');
  const match = /^\[(\d+),(\d+)\]\[(\d+),(\d+)\]$/.exec(node.bounds);
  assert(match, 'The UI control has no usable bounds.');
  const [, left, top, right, bottom] = match.map(Number);
  assert(right > left && bottom > top, 'The UI control is not visible.');
  shell('input', 'tap', String(Math.round((left + right) / 2)), String(Math.round((top + bottom) / 2)));
}
async function tap(text) { tapNode(await find(text)); await sleep(350); }
async function launch() {
  shell('am', 'force-stop', app);
  shell('am', 'start', '-W', '-n', `${app}/.MainActivity`);
  await waitFor(tree => {
    const initial = control(tree, 'Sessions list') ?? control(tree, 'No sessions');
    if (initial) return initial;
    try { shell('pidof', app); }
    catch { throw new Error('Android app exited during startup. Inspect errors.log for the native crash.'); }
    return false;
  }, 'initial Sessions screen');
}
async function settings() { await tap('Settings'); await find('TAILSCALE'); }
async function reveal(text) {
  for (let i = 0; i < 6; i++) {
    const found = control(dump(), text); if (found) return found;
    const dimensions = [...shell('wm', 'size').matchAll(/(?:Physical|Override) size: (\d+)x(\d+)/g)].at(-1);
    assert(dimensions, 'Could not read screen dimensions.');
    const width = Number(dimensions[1]), height = Number(dimensions[2]);
    shell('input', 'swipe', String(Math.round(width / 2)), String(Math.round(height * .75)), String(Math.round(width / 2)), String(Math.round(height * .35)), '400');
  }
  throw new Error(`Could not reach ${text}.`);
}
async function screenshot(name) { writeFileSync(path.join(output, `${name}.png`), adb(['exec-out', 'screencap', '-p'], 20000, 'buffer')); }
async function rename(hostname) {
  assert(/^[a-zA-Z0-9-]+$/.test(hostname), 'Refusing to type an unsafe hostname into adb shell.');
  await tap('Rename');
  const field = await waitFor(tree => tree.find(node => node.type === 'android.widget.EditText'), 'device name field');
  tapNode(field);
  shell('input', 'keycombination', 'KEYCODE_CTRL_LEFT', 'KEYCODE_A');
  shell('input', 'text', hostname);
  // Hide the IME so the save button is visible on short screens.
  shell('input', 'keyevent', 'KEYCODE_BACK');
  await tap('Save');
  await waitFor(tree => !tree.some(node => node.type === 'android.widget.EditText' && node.label === 'Hostname') && tree.some(node => matches(node, hostname)), 'saved device name');
}
async function textSize() {
  // Font controls may be below the fold on small devices.
  for (let i = 0; i < 5; i++) {
    const tree = dump(); const size = tree.find(node => /^\d+ px$/.test(node.text));
    if (size) return Number.parseInt(size.text, 10);
    const screen = /(?:Physical|Override) size: (\d+)x(\d+)/g;
    const dimensions = [...shell('wm', 'size').matchAll(screen)].at(-1);
    assert(dimensions, 'Could not read screen dimensions.');
    const width = Number(dimensions[1]), height = Number(dimensions[2]);
    shell('input', 'swipe', String(Math.round(width / 2)), String(Math.round(height * 0.75)), String(Math.round(width / 2)), String(Math.round(height * 0.35)), '400');
  }
  throw new Error('Terminal text size controls are not reachable.');
}
function redact(text) {
  return text.replace(/https?:\/\/[^\s"<>]+/g, value => { try { return `${new URL(value).origin}/[redacted]`; } catch { return '[redacted-url]'; } })
    .replace(/tskey-[a-zA-Z0-9_-]+/g, '[redacted-key]');
}

let originalName, originalSize, preferencesChanged = false, nameChanged = false;
let originalColors = [], colorsChanged = false;
let failure;
const logSince = shell('date', "'+%m-%d %H:%M:%S.000'").trim();
try {
  assert.equal(shell('getprop', 'sys.boot_completed').trim(), '1', 'Android device is not booted.');
  if (option('--apk')) adb(['install', '-r', path.resolve(option('--apk'))], 180000);
  assert(shell('pm', 'path', app).includes('package:'), 'Install the mobile APK first, or pass --apk.');
  await launch(); await screenshot('sessions');
  assert(!control(dump(), 'Find a session'), 'Search should be hidden initially.');
  await tap('Search sessions'); await find('Find a session');
  await tap('Search sessions');
  assert(!control(dump(), 'Find a session'), 'Search should close when toggled.');
  assert(!control(dump(), 'Machines'), 'The redundant Machines action should not appear.');
  console.log('PASS Compact sessions screen and hidden/toggled search.');
  await settings();
  const tree = dump();
  assert(!tree.some(node => matches(node, 'Sign out')), 'This smoke test must not modify an enrolled device. Use a separate emulator.');
  assert(control(tree, 'Sign in with browser'), 'Expected signed-out settings.');
  await screenshot('settings-initial');
  // Read the actual editable value so an existing user preference can be restored.
  await tap('Rename');
  const nameField = await waitFor(items => items.find(node => node.type === 'android.widget.EditText'), 'current device name');
  originalName = nameField.text; await tap('Cancel');
  assert(/^[a-zA-Z0-9][a-zA-Z0-9-]{0,62}$/.test(originalName), 'Cannot safely preserve the current hostname.');
  originalSize = await textSize();
  const changedSize = originalSize >= 24 ? originalSize - 1 : originalSize + 1;
  preferencesChanged = true;
  await tap(changedSize > originalSize ? '+' : '−'); await find(`${changedSize} px`);
  await launch(); await settings();
  assert.equal(await textSize(), changedSize, 'Font setting did not survive a force-stop/relaunch.');
  await screenshot('font-persisted');
  await tap(changedSize > originalSize ? '−' : '+'); await find(`${originalSize} px`); preferencesChanged = false;
  console.log('PASS Android initial sessions, signed-out settings and font persistence.');
  await launch(); await settings(); await reveal('Use #4ade80');
  originalColors = dump().filter(node => node.selected && /^(?:Use #[0-9a-f]{6}|Use background #[0-9a-f]{6}|Use custom (?:accent|background) #[0-9a-f]{6})$/.test(node.label)).map(node => node.label);
  assert.equal(originalColors.length, 2, 'Could not preserve both selected theme colors.');
  colorsChanged = true;
  await tap('Use #4ade80'); await tap('Use background #ffffff');
  await launch(); await settings(); await reveal('Use #4ade80');
  for (const label of ['Use #4ade80', 'Use background #ffffff']) assert(control(dump(), label)?.selected, 'Theme selection did not persist.');
  await screenshot('light-theme-persisted');
  for (const label of originalColors) await tap(label);
  colorsChanged = false;
  console.log('PASS Accent/background selection and persistence.');
  // Native startup is required for rename and browser login, but not offline UI.
  await launch(); await settings();
  await waitFor(items => {
    if (control(items, 'Retry startup')) throw new Error('Native Tailscale startup failed. Offline UI passed, but rename and browser login are blocked.');
    return control(items, 'Sign in with browser')?.enabled;
  }, 'native networking ready');
  const qaName = originalName === 'shellfox-qa' ? 'shellfox-qa-2' : 'shellfox-qa';
  preferencesChanged = true; nameChanged = true;
  await rename(qaName);
  await launch(); await settings(); await find(qaName); await screenshot('settings-persisted');
  console.log('PASS Device rename persists across force-stop/relaunch.');
  // Restore preferences before opening a browser, where authentication could leave the app.
  await rename(originalName); preferencesChanged = false; nameChanged = false;
  if (options.includes('--browser')) {
    await launch(); await settings(); await tap('Sign in with browser');
    await waitFor(items => {
      const address = items.find(node => node.text && /(?:url_bar|location_bar)$/.test(node.resource));
      if (address) {
        try {
          const url = new URL(address.text.includes('://') ? address.text : `https://${address.text}`);
          if (url.origin === 'https://login.tailscale.com' && !url.username && !url.password && items.some(node => /^Sign in with (?:Google|Microsoft|GitHub|Apple|a passkey)$/.test(node.text))) return true;
        } catch { /* The address bar can briefly show a search hint during navigation. */ }
      }
      if (items.some(node => /Welcome to Chrome|Make Chrome your own|Accept & continue|Use without an account/.test(node.text))) throw new Error('Chrome first-run setup blocks browser verification. Complete browser setup, then rerun --browser.');
      const next = control(items, 'Continue in browser');
      if (next?.enabled) tapNode(next);
      return false;
    }, 'real browser on login.tailscale.com', 90000);
    console.log('PASS Real browser loaded secure Tailscale sign-in page. No credentials entered or enrollment attempted.');
  } else console.log('SKIP Browser launch. Pass --browser to test the real sign-in page.');
  shell('am', 'start', '-W', '-n', `${app}/.MainActivity`);
  await find('TAILSCALE'); await screenshot('settings-restored');
} catch (cause) {
  failure = cause;
  try { if (/mCurrentFocus=.*com\.shellfox\.mobile/.test(shell('dumpsys', 'window', 'windows'))) await screenshot('failure'); }
  catch { /* Crash logs below remain useful if screenshots are unavailable. */ }
}
finally {
  if (colorsChanged && originalColors.length === 2) {
    try { await launch(); await settings(); for (const label of originalColors) tapNode(await reveal(label)); }
    catch { console.error('Could not restore theme colors automatically. Restore them in Settings.'); }
  }
  if (preferencesChanged && originalName && originalSize !== undefined) {
    try {
      await launch(); await settings();
      if (nameChanged) await rename(originalName);
      const currentSize = await textSize();
      for (let i = 0; i < Math.abs(currentSize - originalSize); i++) await tap(currentSize > originalSize ? '−' : '+');
      await find(`${originalSize} px`);
    } catch { console.error('Could not restore preferences automatically. Restore device name and font size in Settings.'); }
  }
  try {
    const logs = adb(['logcat', '-d', '-T', logSince, 'AndroidRuntime:E', 'ReactNativeJS:E', 'Go:E', 'libc:F', '*:S']);
    writeFileSync(path.join(output, 'errors.log'), redact(logs));
    if (/FATAL EXCEPTION|Fatal signal|panic:|fatal error:|ReactNativeJS:.*(?:Error|Exception)/.test(logs)) failure ??= new Error('Android crash or React Native error found. See redacted errors.log.');
  } catch { failure ??= new Error('Could not inspect Android crash logs.'); }
}
console.log(`Artifacts: ${path.relative(root, output)}`);
console.log('Remote terminal E2E not tested. It requires authentic enrollment and host approval.');
if (failure) { console.error(redact(failure instanceof Error ? failure.message : 'Mobile smoke test failed.')); process.exitCode = 1; }
