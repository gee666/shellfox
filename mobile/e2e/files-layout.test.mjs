import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { test } from 'node:test';
import { assertFilesLayout, assertImageToolbar, assertPathSheetLayout, imageActions } from './files-layout.mjs';

const read = name => readFileSync(new URL(name, import.meta.url), 'utf8');
const node = (label, box, resource = '', type = 'android.view.View') => ({ label, text: '', resource, type, bounds: `[${box[0]},${box[1]}][${box[2]},${box[3]}]` });
const metrics = { width: 320, height: 640, scale: 1, insets: { left: 0, top: 24, right: 0, bottom: 24 } };
const controls = { controls: ['Close file', 'More file actions'], last: 'More file actions', directory: true };
const layout = () => [node('', [0, 24, 320, 60], 'files-toolbar'), node('', [0, 60, 320, 616], 'files-content'),
  node('Close file', [2, 26, 34, 58]), node('More file actions', [284, 26, 316, 58]), node('Open folder subdir', [0, 60, 320, 98])];

test('default demo launches the real Files screen without importing QA navigation or helpers', () => {
  assert.match(read('files-entry.js'), /import FilesHarness from '\.\/FilesHarness'/);
  assert.doesNotMatch(read('files-entry.js'), /FilesTestLauncher|files-qa/);
  const demo = read('FilesHarness.tsx');
  assert.match(demo, /useState\(true\)/); assert.match(demo, /<FilesScreen/); assert.match(demo, /label="Browse files"/);
  assert.doesNotMatch(demo, /Open files fixture|Open host files fixture|Stage pending|Read clipboard|FilesTestLauncher/);
  assert.match(read('files-qa-entry.js'), /FilesTestLauncher/);
  assert.match(read('FilesTestLauncher.tsx'), /Files QA launcher/); assert.match(read('FilesTestLauncher.tsx'), /Stage pending download fixture/);
});
test('real-remote cleartext is scoped only to its explicit QA entry and test variant', () => {
  const config = read('remote-android/res/xml/files_remote_qa_network_security.xml');
  assert.match(config, /base-config cleartextTrafficPermitted="false"/);
  assert.match(config, /<domain includeSubdomains="false">10\.0\.2\.2<\/domain>/);
  assert.doesNotMatch(read('../android/app/src/main/AndroidManifest.xml'), /files_remote_qa|10\.0\.2\.2/);
  assert.doesNotMatch(read('../index.js'), /files-remote|RealRemoteFilesHarness|e2e/);
  assert.match(read('build-files-android.mjs'), /System\.getenv\('ENTRY_FILE'\) != 'e2e\/files-remote-entry\.js'/);
  assert.match(read('build-files-android.mjs'), /taskNames != \[':app:assembleTerminalTest'\]/);
});
test('clipboard diagnostics never print or render the original snapshot', () => {
  const helper = read('useQaClipboard.ts'), launcher = read('FilesTestLauncher.tsx');
  assert.match(helper, /useRef/); assert.match(helper, /Clipboard\.getString\(\) === current\.owned/);
  assert.doesNotMatch(helper, /console\.(?:log|info|warn|error)\([^\n]*(?:original|owned)/);
  assert.doesNotMatch(launcher, /Clipboard\.getString/); assert.doesNotMatch(read('FilesHarness.tsx'), /useQaClipboard/);
});
test('one compact toolbar preserves content and full-width folder touch targets', () => {
  assert(assertFilesLayout(layout(), metrics, controls).contentRatio >= .8);
  const narrow = layout(); narrow[narrow.length - 1] = node('Open folder subdir', [0, 60, 130, 98]);
  assert.throws(() => assertFilesLayout(narrow, metrics, controls), /narrow or indented/);
  const clutter = layout(); clutter[0] = node('', [0, 24, 320, 150], 'files-toolbar');
  assert.throws(() => assertFilesLayout(clutter, metrics, controls), /one 36dp row/);
});
test('path alignment measures the actual EditText, never its label-bearing wrapper', () => {
  const rows = [node('File path', [0, 70, 270, 136]), node('File path', [0, 100, 270, 136], '', 'android.widget.EditText'), node('Go', [280, 102, 312, 134])];
  assert.equal(assertPathSheetLayout(rows, metrics).field.top, 100);
  rows[1] = node('File path', [0, 120, 270, 156], '', 'android.widget.EditText');
  assert.throws(() => assertPathSheetLayout(rows, metrics), /vertically aligned/);
});
test('image actions must be one complete row with Close/More first and Save at the far right', () => {
  const rows = [node('', [0, 24, 320, 616], 'files-content'), ...imageActions.map((label, index) => node(label, [index * 35, 26, index * 35 + 32, 58]))];
  assert.equal(assertImageToolbar(rows, metrics).length, 9);
  rows[3] = node(imageActions[2], [70, 64, 102, 96]);
  assert.throws(() => assertImageToolbar(rows, metrics), /share one row/);
});
