import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { existsSync, mkdirSync, writeFileSync } from 'node:fs';
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
      package: node.getAttribute('package') ?? '', selected: node.getAttribute('selected') === 'true', focused: node.getAttribute('focused') === 'true',
    }));
  };
  const matches = (node, text) => node.text.toLowerCase() === text.toLowerCase() || node.label.toLowerCase() === text.toLowerCase();
  const wait = async (predicate, description, timeout = 60_000) => {
    const until = Date.now() + timeout;
    while (Date.now() < until) { const result = predicate(dump()); if (result) return result; await sleep(250); }
    throw new Error(`Missing Android UI: ${description}`);
  };
  const stable = (text, enabled, samples) => {
    let previous = '', count = 0;
    return wait(nodes => {
      const node = nodes.find(item => item.package === app && item.label.toLowerCase() === text.toLowerCase()) ?? nodes.find(item => item.label.toLowerCase() === text.toLowerCase()) ?? nodes.find(item => item.text.toLowerCase() === text.toLowerCase());
      if (!node || !controlBounds(node) || enabled && !node.enabled) { previous = ''; count = 0; return false; }
      const route = nodes.find(item => item.resource === 'files-title')?.text ?? '';
      const current = `${node.bounds}|${node.resource}|${node.label}|${route}`;
      count = current === previous ? count + 1 : 1; previous = current;
      return count >= samples ? node : false;
    }, `${text}, stable native bounds`);
  };
  const find = text => stable(text, false, 2);
  const tapNode = node => { const box = controlBounds(node); assert(box, 'Control is off screen.'); shell('input', 'tap', String(Math.round((box.left + box.right) / 2)), String(Math.round((box.top + box.bottom) / 2))); };
  const tap = async text => { tapNode(await stable(text, true, 3)); };
  const tapFor = async (text, ready) => {
    const route = dump().find(node => node.resource === 'files-title')?.text ?? '';
    for (let attempt = 0; attempt < 2; attempt++) {
      const current = dump(); if (ready(current)) return;
      await tap(text);
      try { return await wait(ready, `${text} navigation outcome`, 4000); }
      catch (error) {
        const now = dump();
        if (attempt || (now.find(node => node.resource === 'files-title')?.text ?? '') !== route || !now.some(node => matches(node, text) && node.enabled)) throw error;
      }
    }
  };
  const reveal = async (text, id = 'files-content') => {
    for (const direction of ['forward', 'backward']) for (let attempt = 0; attempt < 6; attempt++) {
      if (dump().some(node => matches(node, text) && controlBounds(node))) return find(text);
      const container = dump().find(node => node.resource === id && controlBounds(node));
      const box = controlBounds(container); assert(box, `Missing scroll container: ${id}`);
      // Stay away from Android's bottom home-gesture strip and side back-gesture edges.
      const top = Math.round(box.top + (box.bottom - box.top) * .25), bottom = Math.round(box.top + (box.bottom - box.top) * .75), x = Math.round((box.left + box.right) / 2);
      shell('input', 'swipe', String(x), String(direction === 'forward' ? bottom : top), String(x), String(direction === 'forward' ? top : bottom), '400');
    }
    throw new Error(`Control is unreachable after bounded scrolling: ${text}`);
  };
  const screenshot = name => writeFileSync(path.join(output, `${name}.png`), adb(['exec-out', 'screencap', '-p'], 'buffer'));
  const assertNoQaTools = () => {
    assert(!dump().some(node => /^(?:Files QA launcher|Open (?:host )?files fixture|Stage pending download fixture|Read clipboard fixture)$/.test(node.label || node.text)), 'The demo must not expose QA tools as app navigation.');
  };
  let runningQa = false, cleanupSequence = 0;
  const launch = async (mode = 'qa') => {
    assert(['qa', 'demo', 'remote'].includes(mode), 'Choose an explicit QA, real-remote QA or demo entry.');
    if (runningQa) await cleanupQa();
    runningQa = false; shell('am', 'force-stop', app); shell('am', 'start', '-W', '-n', `${app}/com.shellfox.mobile.MainActivity`);
    await find(mode === 'qa' ? 'Files QA launcher' : mode === 'remote' ? 'Real remote host' : 'Open file code.ts');
    runningQa = mode !== 'demo';
    if (mode === 'demo') assertNoQaTools();
    else await wait(() => /FILES_QA_CLIPBOARD_READY (?:true|false)/.test(qaLogs()), 'memory-only QA clipboard snapshot');
  };
  const install = apk => {
    const aapt = path.join(sdk, 'build-tools/36.0.0', process.platform === 'win32' ? 'aapt.exe' : 'aapt');
    const manifest = execFileSync(aapt, ['dump', 'badging', apk], { encoding: 'utf8', timeout: 30_000 });
    assert.match(manifest, /^package: name='com\.shellfox\.mobile\.terminaltest' /m, 'Refusing a production APK.');
    adb(['install', '-r', apk], 'utf8', 180_000);
  };
  const qaLogs = () => {
    const pid = shell('pidof', app).trim(); if (!/^\d+$/.test(pid)) return '';
    return adb(['logcat', '-d', `--pid=${pid}`, '-v', 'brief', 'ReactNativeJS:I', '*:S']);
  };
  const qaClipboardReady = () => qaLogs().includes('FILES_QA_CLIPBOARD_READY true');
  const cleanupQa = async () => {
    if (!runningQa || !shell('pidof', app).trim()) return;
    const token = `${Date.now().toString(36)}-${++cleanupSequence}`;
    shell('am', 'start', '-W', '-a', 'android.intent.action.VIEW', '-d', `shellfox-files-qa://cleanup?token=${token}`, '-n', `${app}/com.shellfox.mobile.MainActivity`);
    await wait(() => qaLogs().includes(`FILES_QA_CLEANUP ${token}`), 'QA clipboard cleanup acknowledgement', 15_000);
  };
  const viewport = () => {
    const logs = qaLogs();
    const records = [...logs.matchAll(/FILES_QA_VIEWPORT (\{[^\r\n]+\})/g)];
    try { return records.length ? JSON.parse(records.at(-1)[1]) : undefined; } catch { return undefined; }
  };
  const qaReported = message => qaLogs().includes(`FILES_QA_STATUS ${JSON.stringify(message)}`);
  const qaCommand = command => {
    assert(['disconnect', 'reconnect'].includes(command), 'Unknown explicit QA command.');
    shell('am', 'start', '-W', '-a', 'android.intent.action.VIEW', '-d', `shellfox-files-qa://${command}`, '-n', `${app}/com.shellfox.mobile.MainActivity`);
  };
  const leaveToQa = async () => {
    for (let attempt = 0; attempt < 6; attempt++) {
      if (dump().some(node => matches(node, 'Files QA launcher'))) return;
      shell('input', 'keyevent', 'KEYCODE_BACK'); await sleep(300);
    }
    throw new Error('Files did not return to its explicit QA launcher.');
  };
  const qaApk = path.resolve(process.argv[2] ?? path.join(root, 'tmp/mobile-e2e/files-qa.apk'));
  const remoteApk = path.join(root, 'tmp/mobile-e2e/files-remote.apk');
  const demoApk = path.resolve(process.env.SHELLFOX_FILES_DEMO_APK ?? path.join(root, 'tmp/mobile-e2e/files-demo.apk'));
  const requireApks = () => { assert(existsSync(qaApk), `Build the explicit QA entry first: ${qaApk}`); assert(existsSync(demoApk), `Build the default demo entry for restoration: ${demoApk}`); };
  const restoreDemo = async () => {
    await cleanupQa(); runningQa = false; install(demoApk); await launch('demo');
    await tap('Open folder subdir'); await tap('Open file nested.txt'); await tap('Close file'); await find('Open file nested.txt');
    await tap('Back'); await find('Open file code.ts'); await tap('Back'); await find('Browse files'); assertNoQaTools(); screenshot('demo-workspace');
    await tap('Browse files'); await find('Open file code.ts'); assertNoQaTools(); screenshot('final-demo-files');
  };
  return { root, app, output, adb, shell, dump, matches, wait, find, tapNode, tap, screenshot, launch, install, qaApk, demoApk, remoteApk, requireApks, restoreDemo, assertNoQaTools, viewport, qaCommand, leaveToQa, qaReported, cleanupQa, qaClipboardReady, reveal, tapFor };
}
