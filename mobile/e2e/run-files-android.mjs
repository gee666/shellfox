// Real React Native controls/WebViews, explicit QA entry, then restore the clean demo entry.
import assert from 'node:assert/strict';
import { writeFileSync } from 'node:fs';
import path from 'node:path';
import { setTimeout as sleep } from 'node:timers/promises';
import { keyboardIsShown, controlBounds } from '../../scripts/mobile-e2e-controls.mjs';
import { androidFilesDriver } from './android-files-driver.mjs';
import { assertFilesLayout, assertImageToolbar, assertPathSheetLayout, byId } from './files-layout.mjs';

const d = androidFilesDriver('mobile-files-native');
d.requireApks();
const captures = [], layouts = [];
const originalSize = /Override size:\s*(\d+x\d+)/.exec(d.shell('wm', 'size'))?.[1];
const originalDensity = /Override density:\s*(\d+)/.exec(d.shell('wm', 'density'))?.[1];
const originalIme = d.shell('settings', 'get', 'secure', 'show_ime_with_hard_keyboard').trim();
function restoreIme() { if (originalIme === 'null') d.shell('settings', 'delete', 'secure', 'show_ime_with_hard_keyboard'); else d.shell('settings', 'put', 'secure', 'show_ime_with_hard_keyboard', originalIme); }
const capture = (name, label, review = false) => {
  d.screenshot(name); writeFileSync(path.join(d.output, `${name}-ui.json`), JSON.stringify(d.dump(), null, 2));
  if (review) captures.push({ label, path: path.join(d.output, `${name}.png`) });
};
async function openRoot(host = false) { await d.launch('qa'); await d.tap(host ? 'Open host files fixture' : 'Open files fixture'); await d.find('Open file code.ts'); }
async function leaveToQa() {
  for (let attempt = 0; attempt < 6; attempt++) {
    if (d.dump().some(node => d.matches(node, 'Files QA launcher'))) return;
    d.shell('input', 'keyevent', 'KEYCODE_BACK'); await sleep(300);
  }
  throw new Error('Files did not return to the explicit QA launcher.');
}
async function enter(text) {
  assert(/^[A-Za-z]+$/.test(text)); d.tapNode(await d.find('File source'));
  d.shell('input', 'keycombination', 'KEYCODE_CTRL_LEFT', 'KEYCODE_A'); d.shell('input', 'text', text); await d.find(text);
}
async function imageStatus(dirty) {
  return d.wait(nodes => nodes.find(node => /^\d+ × \d+(?: · Unsaved)?$/.test(node.text) && node.text.includes('Unsaved') === dirty)?.text, `image state dirty=${dirty}`);
}
async function copyFixturePath() {
  if (d.qaClipboardReady()) { await d.tap('Copy file path'); return true; }
  console.log('SKIP Native clipboard copy: original device clipboard is not restorable text; it was left untouched.'); return false;
}
async function dimensions(width, height) {
  return d.wait(() => { const metrics = d.viewport(); return metrics && Math.abs(metrics.width - width) < 1 && Math.abs(metrics.height - height) < 1 ? metrics : false; }, `${width}x${height}dp native viewport`);
}
function checkLayout(name, controls, last, directory = false) {
  const metrics = d.viewport(), result = assertFilesLayout(d.dump(), metrics, { controls, last, directory });
  layouts.push({ name, metrics, ...result }); return result;
}
async function revealPathControl(label) {
  for (const direction of ['forward', 'backward']) for (let attempt = 0; attempt < 5; attempt++) {
    const visible = d.dump().find(node => d.matches(node, label) && controlBounds(node)); if (visible) return visible;
    const box = controlBounds(byId(d.dump(), 'files-path-sheet')); assert(box, 'Path sheet closed while revealing a control.');
    const top = Math.round(box.top + (box.bottom - box.top) * .25), bottom = Math.round(box.top + (box.bottom - box.top) * .75), x = Math.round((box.left + box.right) / 2);
    d.shell('input', 'swipe', String(x), String(direction === 'forward' ? bottom : top), String(x), String(direction === 'forward' ? top : bottom), '350');
  }
  throw new Error(`Path sheet control is unreachable with the software keyboard open: ${label}`);
}
async function requireSoftwareKeyboard() {
  return d.wait(nodes => {
    const state = d.shell('dumpsys', 'input_method');
    if (!keyboardIsShown(state)) return false;
    assert(!/\b(?:mIsFullscreen|mInFullscreenMode|isFullscreenMode)=true\b/.test(state), 'Fullscreen IME extraction covers app controls; backing app bounds are not a visual pass.');
    assert(!nodes.some(node => /extract(?:EditText|_edit_text)/i.test(node.resource) && controlBounds(node)), 'IME extract editor replaced the app path sheet.');
    const scale = d.viewport()?.scale ?? 3;
    const keyboard = nodes.filter(node => /inputmethod/.test(node.package)).map(controlBounds).filter(Boolean).find(box => box.bottom - box.top >= 120 * scale);
    if (keyboard) return keyboard;
    const windows = d.shell('dumpsys', 'window');
    const frames = [...windows.matchAll(/type=ime[^\r\n]*frame=(\[\d+,\d+\]\[\d+,\d+\])[^\r\n]*visible=true/g)];
    return frames.map(match => controlBounds({ bounds: match[1] })).find(box => box && box.bottom - box.top >= 120 * scale) ?? false;
  }, 'full software keyboard, not only the hardware-keyboard toolbar');
}
async function geometry() {
  const requested = process.env.SHELLFOX_FILES_GEOMETRY_WIDTHS?.split(',').map(Number);
  if (requested) assert(requested.length && requested.every(width => [320, 360, 640].includes(width)), 'Geometry widths must be 320, 360 or 640.');
  const targets = [[320, 640, 'portrait'], [360, 720, 'portrait'], [640, 360, 'landscape']].filter(([width]) => !requested || requested.includes(width));
  for (const [width, height, orientation] of targets) {
    await d.cleanupQa();
    d.shell('wm', 'size', `${width * 3}x${height * 3}`); d.shell('wm', 'density', '480');
    await openRoot(true); const metrics = await dimensions(width, height), prefix = `${width}-${orientation}`;
    checkLayout(`${prefix}-directory`, ['Back', 'Change directory', 'Upload to this folder', 'Refresh', 'More folder actions'], 'More folder actions', true);
    capture(`${prefix}-directory`, `${width}dp ${orientation}, directory`, true);
    const folder = controlBounds(await d.reveal('Open folder subdir')); assert(folder);
    d.shell('input', 'tap', String(folder.right - Math.ceil(metrics.scale * 3)), String(Math.round((folder.top + folder.bottom) / 2)));
    await d.find('Open file nested.txt'); await d.tap('Back'); await d.reveal('Open file code.ts');
    await d.tap('Open file code.ts'); await d.find('Code preview');
    checkLayout(`${prefix}-read`, ['Close file', 'Edit source', 'Show file path', 'Copy file path', 'Download selected file', 'More file actions'], 'More file actions');
    capture(`${prefix}-read`, `${width}dp ${orientation}, wrapped code`, true);
    await copyFixturePath();
    if (process.env.SHELLFOX_FILES_SKIP_PATH === '1') console.log('SKIP Path/keyboard checks explicitly disabled for this isolated layout capture; this run does not verify them.');
    else {
    if (orientation === 'landscape') d.shell('settings', 'put', 'secure', 'show_ime_with_hard_keyboard', '1');
    await d.tap('Show file path'); await d.find('File path');
    if (orientation === 'landscape') {
      d.tapNode(await d.wait(nodes => nodes.find(node => node.type === 'android.widget.EditText' && node.label === 'File path'), 'path EditText'));
      const keyboard = await requireSoftwareKeyboard();
      await revealPathControl('Go');
      layouts.push({ name: `${prefix}-path-keyboard`, keyboard, metrics: d.viewport(), ...assertPathSheetLayout(d.dump(), d.viewport(), keyboard) });
      capture(`${prefix}-path-keyboard-go`, 'Landscape actual EditText and Go icon above keyboard');
      await d.tap('Go'); await d.find('Code preview');
      await d.tap('Show file path'); d.tapNode(await d.wait(nodes => nodes.find(node => node.type === 'android.widget.EditText' && node.label === 'File path'), 'path EditText'));
      await requireSoftwareKeyboard();
      d.shell('input', 'keycombination', 'KEYCODE_CTRL_LEFT', 'KEYCODE_A'); d.shell('input', 'text', '/fixture/s');
      const suggestion = await revealPathControl('/fixture/subdir/'); capture(`${prefix}-path-keyboard-suggestions`, 'Landscape path suggestions with full software keyboard');
      d.tapNode(suggestion);
      if (byId(d.dump(), 'files-path-sheet')) {
        await revealPathControl('Go');
        assertPathSheetLayout(d.dump(), d.viewport(), await requireSoftwareKeyboard()); await d.tap('Go');
      }
      await d.find('Open file nested.txt'); restoreIme(); await d.tap('Back'); await d.tap('Open file code.ts'); await d.find('Code preview');
    } else {
      layouts.push({ name: `${prefix}-path`, metrics: d.viewport(), ...assertPathSheetLayout(d.dump(), d.viewport()) });
      capture(`${prefix}-path`, `${width}dp path sheet`); await d.tap('Close path'); await d.find('Code preview');
    }
    }
    await d.tap('Edit source'); await d.find('File source');
    checkLayout(`${prefix}-edit`, ['Close file', 'Read file', 'Show file path', 'Copy file path', 'More file actions', 'Save'], 'Save');
    capture(`${prefix}-edit`, `${width}dp edit source`);
    await d.tapFor('More file actions', nodes => byId(nodes, 'files-menu')); await d.find('Unwrap lines'); await d.tap('Unwrap lines');
    await d.tapFor('More file actions', nodes => byId(nodes, 'files-menu')); await d.find('Wrap lines'); await d.tap('Wrap lines');
    await d.tap('Close file'); await d.find('Open file image.png');
    await d.tap('Open file image.png'); await d.tap('Edit image'); await d.find('Save image'); await imageStatus(false);
    layouts.push({ name: `${prefix}-image`, metrics: d.viewport(), actions: assertImageToolbar(d.dump(), d.viewport()) });
    capture(`${prefix}-image`, `${width}dp ${orientation}, all 9 image actions`, true);
    await d.tapFor('More file actions', nodes => byId(nodes, 'files-menu'));
    d.shell('input', 'keyevent', 'KEYCODE_BACK'); await d.find('Image canvas');
    await d.tap('Close file'); await d.find('Open file code.ts');
  }
  console.log(`PASS ${targets.map(([width, _height, orientation]) => `${width}dp ${orientation}`).join(', ')} native geometry, full-width row touches, centered transfer glyphs, >=80% content and nine image actions${process.env.SHELLFOX_FILES_SKIP_PATH === '1' ? '; path checks explicitly skipped' : ', including visible native path/Go checks'}.`);
}
let failure;
try {
  assert.equal(d.shell('getprop', 'sys.boot_completed').trim(), '1'); d.install(d.qaApk);
  if (process.env.SHELLFOX_FILES_FUNCTIONAL_ONLY !== '1') await geometry();
  if (process.env.SHELLFOX_FILES_GEOMETRY_ONLY !== '1') {
  await d.cleanupQa(); restoreIme(); d.shell('wm', 'size', originalSize ?? 'reset'); d.shell('wm', 'density', originalDensity ?? 'reset');
  await openRoot(); capture('files-directory', 'Files directory');
  if (process.env.SHELLFOX_FILES_IMAGE_ONLY !== '1') {
    await d.tap('Open folder subdir'); await d.find('Open file nested.txt');
    assert(!d.dump().some(node => d.matches(node, 'Open file code.ts')), 'Folder navigation must not keep the parent listing.');
    await d.tap('Back'); await d.tap('Open file code.ts'); await d.find('Code preview'); capture('code-highlighted', 'Code preview');
    const copied = await copyFixturePath(); await d.tap('Close file'); await leaveToQa();
    if (copied) { await d.tap('Read clipboard fixture'); await d.find('Clipboard: /fixture/code.ts'); console.log('PASS Canonical fixture path copied to the real Android clipboard.'); }
    console.log('PASS Session/tab folder navigation and native syntax preview.');
    await d.tap('Open files fixture'); await d.tap('Open file code.ts'); await d.tap('Edit source'); await enter('NativeSaved');
    const savedText = process.env.SHELLFOX_FILES_SKIP_CARET === '1' ? 'NativeSaved' : 'NaXtiveSaved';
    if (process.env.SHELLFOX_FILES_SKIP_CARET === '1') console.log('SKIP Known native caret restore bug on this APK; tracked separately, not a pass.');
    else {
      d.shell('input', 'keyevent', 'KEYCODE_MOVE_HOME'); d.shell('input', 'keyevent', 'KEYCODE_DPAD_RIGHT'); d.shell('input', 'keyevent', 'KEYCODE_DPAD_RIGHT');
      await d.tap('Read file'); await d.find('Code preview'); await d.tap('Edit source'); await d.find('File source');
      d.shell('input', 'text', 'X'); await d.find('NaXtiveSaved');
      console.log('PASS Native source caret survives Read/Edit and inserts at index 2 without Ctrl+A.');
    }
    await d.tap('Save'); await d.wait(nodes => nodes.find(node => node.label === 'Save' && !node.enabled), 'saved editor'); capture('native-source-saved', 'Saved source');
    await d.tap('Close file'); await leaveToQa(); await d.find(`Saved text: ${savedText}`);
    await d.tap('Open files fixture'); await d.tap('Open file code.ts'); await d.tap('Edit source'); await enter('UnsavedNative');
    await d.tap('Close file'); await d.find('Discard changes?'); await d.tap('Keep editing'); await d.find('UnsavedNative');
    await d.tap('Close file'); await d.tap('Discard'); await d.find('Open file README.md');
    console.log('PASS Native source edit/save and dirty Close keep/discard return to the directory, not test navigation.');
    await d.tap('Open file README.md'); await d.find('Native Markdown'); await d.find('Markdown sibling loaded'); capture('markdown-sibling', 'Markdown sibling image');
    await d.tap('Close file'); await leaveToQa(); await d.find('Loaded asset: image.png');
    await d.tap('Open files fixture'); await d.tap('Open file index.html'); await d.tap('Run preview'); await d.find('123'); capture('interactive-html', 'Interactive HTML');
    await d.tap('Close file'); await d.tap('Open file icon.svg'); await d.find('SVG preview'); await d.tap('Edit source'); await d.find('File source');
    await d.tap('Close file'); await d.tap('Open file icon.ico'); await d.find('Image preview'); capture('ico-preview', 'ICO preview'); await d.tap('Close file');
    console.log('PASS Real Android WebView Markdown/HTML sibling assets, JavaScript interaction, SVG source and ICO preview.');
    await openRoot(true); await d.tap('Open file qa-slow.txt'); await d.find('Close file'); await d.tap('Close file'); await d.find('Open file code.ts');
    await d.wait(() => d.qaReported('Slow fixture read finished'), 'closed delayed read settled');
    await d.find('Open file code.ts'); assert(!d.dump().some(node => d.matches(node, 'Code preview')), 'A closed pending read must not replace its cached directory when it resolves.');
    await d.tap('Open file shellfox-transfer-large.bin'); await d.find('Close file'); await d.find('Fixture preview is too large: 67108881 bytes.');
    await d.tap('Close file'); await d.find('Open file code.ts');
    console.log('PASS Loading/unsupported file targets retain Close chrome; late reads cannot reopen a closed file.');
    await d.tap('Open folder subdir'); await d.tap('Open file nested.txt'); await d.find('Code preview');
    d.qaCommand('disconnect'); await d.wait(nodes => nodes.find(node => node.label === 'Download selected file' && !node.enabled), 'offline file toolbar');
    await d.tap('Close file'); await d.find('Open file nested.txt'); capture('offline-close-directory', 'Offline close returns to its origin folder');
    assert(!d.dump().some(node => d.matches(node, 'Files QA launcher')), 'Offline Close must retain the origin directory snapshot.');
    d.qaCommand('reconnect'); await d.wait(nodes => nodes.find(node => node.label === 'Open file nested.txt' && node.enabled), 'reconnected folder');
    await d.tap('Back'); await d.tap('Open file code.ts'); await copyFixturePath(); await d.tap('Show file path');
    const field = await d.wait(nodes => nodes.find(node => node.type === 'android.widget.EditText' && node.label === 'File path'), 'actual path EditText');
    d.tapNode(field); d.shell('input', 'keycombination', 'KEYCODE_CTRL_LEFT', 'KEYCODE_A'); d.shell('input', 'text', '/fixture/s'); await d.find('/fixture/subdir/');
    d.shell('input', 'keyevent', 'KEYCODE_ESCAPE'); await sleep(300);
    assert(!d.dump().some(node => d.matches(node, '/fixture/subdir/')), 'First Escape should dismiss suggestions.');
    d.shell('input', 'keyevent', 'KEYCODE_ESCAPE'); await d.find('Code preview');
    assert(!byId(d.dump(), 'files-path-sheet'), 'Second Escape should close the path sheet, not the file.');
    await d.tapFor('More file actions', nodes => byId(nodes, 'files-menu')); await d.find('Unwrap lines'); d.shell('input', 'keyevent', 'KEYCODE_BACK'); await d.find('Code preview');
    console.log('PASS Offline file Close uses the cached origin folder; path suggestions/sheet/menu navigation do not close the file.');
    await d.tap('Close file');
  }
  await d.tap('Open file image.png'); await d.tap('Edit image'); await imageStatus(false); await d.tap('Rotate right');
  const rotated = await imageStatus(true); capture('image-rotated', 'Rotated image');
  const canvas = controlBounds(await d.find('Image canvas')); assert(canvas, 'Image canvas must be visible for a real touch crop.');
  d.shell('input', 'swipe', String(Math.round(canvas.left + (canvas.right - canvas.left) * .2)), String(Math.round(canvas.top + (canvas.bottom - canvas.top) * .2)),
    String(Math.round(canvas.left + (canvas.right - canvas.left) * .8)), String(Math.round(canvas.top + (canvas.bottom - canvas.top) * .8)), '700');
  capture('image-crop-selection', 'Touch crop selection'); await d.tap('Crop'); assert.notEqual(await imageStatus(true), rotated, 'Touch cropping must change canvas dimensions.');
  await d.tap('Save image'); await imageStatus(false); capture('image-saved', 'Saved image'); await d.tap('Close file'); await leaveToQa(); await d.find('Saved image');
  console.log('PASS Android image child toolbar, rotate, real touch crop and authenticated Save image.');
  }
} catch (cause) {
  failure = cause; writeFileSync(path.join(d.output, 'failure-ui.json'), JSON.stringify(d.dump(), null, 2)); d.screenshot('failure');
} finally {
  await d.cleanupQa().catch(() => {});
  restoreIme(); d.shell('wm', 'size', originalSize ?? 'reset'); d.shell('wm', 'density', originalDensity ?? 'reset');
  writeFileSync(path.join(d.output, 'layouts.json'), JSON.stringify(layouts, null, 2)); writeFileSync(path.join(d.output, 'captures.json'), JSON.stringify(captures, null, 2));
  const pid = d.shell('pidof', d.app).trim();
  if (pid) {
    const logs = d.adb(['logcat', '-d', `--pid=${pid}`, '-v', 'brief', 'AndroidRuntime:E', 'ReactNativeJS:E', 'chromium:E', '*:S']); writeFileSync(path.join(d.output, 'app.log'), logs);
    if (/FATAL EXCEPTION|(?:^|\n)E\/ReactNativeJS/.test(logs)) failure ??= new Error('Native runtime error. Inspect app.log.');
  }
  try {
    if (process.env.SHELLFOX_FILES_KEEP_QA === '1') { await d.cleanupQa(); console.log('Explicit QA build retained for visual review; run run-files-demo-android.mjs after signoff.'); }
    else { await d.restoreDemo(); console.log('PASS Demo browser-first landing, file Close returns to its folder, root Back opens compact workspace with no QA tools.'); }
  } catch (cause) { failure ??= cause; }
}
console.log(`Artifacts: ${path.relative(d.root, d.output)}`);
if (failure) { console.error(failure); process.exitCode = 1; }
