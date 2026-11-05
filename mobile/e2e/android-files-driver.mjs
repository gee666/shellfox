import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { mkdirSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { setTimeout as sleep } from 'node:timers/promises';
import { DOMParser } from '@xmldom/xmldom';
import { controlBounds, parseHierarchy } from '../../scripts/mobile-e2e-controls.mjs';

export function androidFilesDriver(folder = 'mobile-file-transfers') {
  const root = fileURLToPath(new URL('../../', import.meta.url));
  const sdk = process.env.ANDROID_HOME; assert(sdk, 'ANDROID_HOME is required.');
  const serial = process.env.ANDROID_SERIAL ?? 'emulator-5554'; assert(/^emulator-\d+$/.test(serial), 'Only an isolated emulator may run Files tests.');
  const app = 'com.shellfox.mobile.terminaltest';
  const output = path.join(root, 'tmp', folder, new Date().toISOString().replace(/[:.]/g, '-')); mkdirSync(output, { recursive: true });
  const adbPath = path.join(sdk, 'platform-tools', process.platform === 'win32' ? 'adb.exe' : 'adb');
  const adb = (args, encoding = 'utf8', timeout = 30_000) => execFileSync(adbPath, ['-s', serial, ...args], { encoding, timeout, windowsHide: true, maxBuffer: 16 * 1024 * 1024 });
  const shell = (...args) => adb(['shell', ...args]);
  const dump = () => {
    const xml = parseHierarchy(adb(['exec-out', 'uiautomator', 'dump', '/dev/tty']));
    if (!xml) return [];
    return Array.from(new DOMParser().parseFromString(xml, 'text/xml').getElementsByTagName('node')).map(node => ({
      text: node.getAttribute('text') ?? '', label: node.getAttribute('content-desc') ?? '', bounds: node.getAttribute('bounds') ?? '',
      resource: node.getAttribute('resource-id') ?? '', type: node.getAttribute('class') ?? '', enabled: node.getAttribute('enabled') !== 'false',
    }));
  };
  const matches = (node, text) => node.text.toLowerCase() === text.toLowerCase() || node.label.toLowerCase() === text.toLowerCase();
  const wait = async (predicate, description, timeout = 60_000) => {
    const until = Date.now() + timeout;
    while (Date.now() < until) { const result = predicate(dump()); if (result) return result; await sleep(250); }
    throw new Error(`Missing Android UI: ${description}`);
  };
  const find = text => wait(nodes => nodes.find(node => matches(node, text)), text);
  const tapNode = node => { const box = controlBounds(node); assert(box, 'Control is off screen.'); shell('input', 'tap', String(Math.round((box.left + box.right) / 2)), String(Math.round((box.top + box.bottom) / 2))); };
  const tap = async text => { tapNode(await wait(nodes => {
    const node = nodes.find(item => item.label.toLowerCase() === text.toLowerCase()) ?? nodes.find(item => item.text.toLowerCase() === text.toLowerCase());
    return node?.enabled ? node : false;
  }, text)); await sleep(250); };
  const screenshot = name => writeFileSync(path.join(output, `${name}.png`), adb(['exec-out', 'screencap', '-p'], 'buffer'));
  const launch = async () => { shell('am', 'force-stop', app); shell('am', 'start', '-W', '-n', `${app}/com.shellfox.mobile.MainActivity`); await find('Open files fixture'); };
  const install = apk => {
    const aapt = path.join(sdk, 'build-tools/36.0.0', process.platform === 'win32' ? 'aapt.exe' : 'aapt');
    const manifest = execFileSync(aapt, ['dump', 'badging', apk], { encoding: 'utf8', timeout: 30_000 });
    assert.match(manifest, /^package: name='com\.shellfox\.mobile\.terminaltest' /m, 'Refusing a production APK.');
    adb(['install', '-r', apk], 'utf8', 180_000);
  };
  return { root, app, output, adb, shell, dump, matches, wait, find, tapNode, tap, screenshot, launch, install };
}
