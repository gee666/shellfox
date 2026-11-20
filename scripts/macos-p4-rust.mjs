import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { createWriteStream } from 'node:fs';
import { readFile, rename, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { setTimeout as delay } from 'node:timers/promises';
import { fileHash, run } from './rust-package-common.mjs';
import { marker, original, updated } from './macos-p4-fixture.mjs';
import { eventually, nativePicker } from './macos-p4-dialog.mjs';
import { exerciseTransfers } from './macos-p4-transfers.mjs';
import { peerEnv, startRustPeer, viewerFlags } from './macos-p4-rust-peer.mjs';

const keyboard = { a: 0, s: 1, z: 6, x: 7, c: 8, b: 11, q: 12, e: 14,
  enter: 36, escape: 53, right: 124, down: 125, up: 126 };
const named = (tree, name) => tree.find(node => node.title === name || node.description === name);
const observed = (shot, value) => shot.lines.find(row => row.text.toLowerCase().includes(value.toLowerCase()));

export async function runRust({ binary, cli, helper, testcontrol, project, dir, env, window, checkNetwork }) {
  const data = path.join(dir, 'host');
  const argv = ['--shell', '--data-dir', data, '--desktop-dir', path.join(dir, 'desktop'),
    '--device-config-root', path.join(dir, 'device'), '--helper', helper, '--helper-sha256', fileHash(helper), ...viewerFlags];
  const child = spawn(binary, argv, { cwd: project, env: { ...env, ...peerEnv }, stdio: ['ignore', 'pipe', 'pipe'] });
  const output = createWriteStream(path.join(dir, 'process.log'));
  child.stdout.pipe(output, { end: false });
  child.stderr.pipe(output, { end: false });
  const exited = new Promise((resolve, reject) => {
    child.once('error', reject);
    child.once('close', (code, signal) => { output.end(); resolve({ code, signal }); });
  });
  exited.catch(() => {});
  const action = (...args) => window(child.pid, binary, '--data-dir', data, ...args);
  const tree = () => action('tree');
  const picker = nativePicker(action);
  const key = (id, modifiers = '-') => action('key', keyboard[id], modifiers);
  const click = ({ x, y }) => action('click', x, y);
  const node = (name, apply = value => Boolean(value)) => eventually(name,
    () => named(tree(), name), apply);
  const editor = expected => node('File contents', field => field?.value === expected);
  const clickNode = ({ rect }) => click({ x: rect.x + rect.width / 2, y: rect.y + rect.height / 2 });
  const shot = (name, want = () => true) => eventually(`Rust screenshot ${name}`, () => action('capture', path.join(dir, `${name}.png`)), want);
  const clickText = async (name, match = shot => observed(shot, name)) => click(match(await shot(`before-${name.replace(/[^a-zA-Z0-9-]/g, '_')}`, match)));
  const fillPath = async value => {
    clickNode(await node('Folder path'));
    await node('Folder path', field => field?.focused === true);
    key('a', 'c');
    action('paste', value);
    await node('Folder path', field => field?.value === value);
  };
  const go = async (value, folder) => {
    await fillPath(value);
    key('escape');
    key('enter');
    if (folder) await node('Folder path', field => field?.value === folder);
  };
  const headerControl = async (inset) => {
    const { rect } = await node('File sidebar');
    click({ x: rect.x + rect.width - inset, y: rect.y + 16 });
  };
  const browse = async () => {
    const { rect } = await node('Folder path');
    click({ x: rect.x + rect.width + 15, y: rect.y + rect.height / 2 });
    return picker.wait();
  };
  const togglePreview = async () => {
    const { rect } = await node('File sidebar');
    // The last two 24px toolbar controls are Source and Preview.
    const folder = named(tree(), 'Folder path');
    assert.ok(!folder, 'Collapse the browser to make the document toolbar position unambiguous');
    const press = farRight => click({ x: rect.x + rect.width - farRight, y: rect.y + 32 + 17 });
    return { source: () => press(43), preview: () => press(16) };
  };
  let failure, peer;
  try {
    await Promise.race([exited.then(({ code }) => { throw new Error(`Rust desktop exited early: ${code}`); }),
      eventually('owned Rust Aqua window', () => {
        try { return tree(); } catch { return []; }
      }, rows => rows.length > 1, 55_000)]);
    action('activate');
    action('resize-content', 40, 40, 1200, 760);
    const startup = await shot('01-startup');
    assert.deepEqual(startup.contentSize, [1200, 760]);
    await node('Files', value => value?.enabled === true);
    action('press-control', 'Files');
    const pane = await node('File sidebar');
    assert.equal(pane.rect.width, 420);
    await go(project, project);
    clickNode(await node('Folder path'));
    key('b', 'cs');
    await node('File sidebar', value => !value);
    await node('Files', value => value?.focused === true);
    key('b', 'cs');
    await node('Folder path', field => field?.value === project);
    await shot('02-files', shot => observed(shot, 'notes.txt'));
    const dialog = await browse();
    await writeFile(path.join(dir, 'native-folder-picker.json'), JSON.stringify(dialog, null, 2));
    await picker.cancel();
    await node('Folder path', field => field?.value === project);
    await browse();
    const unique = path.join(project, 'unique-folder');
    await picker.choose(unique);
    await node('Folder path', field => field?.value === unique);
    await go(project, project);
    key('e', 'cs');
    await node('Folder path', field => !field);
    key('e', 'cs');
    await node('Folder path');

    await fillPath(path.join(project, 'uni'));
    await shot('03-suggestion', shot => observed(shot, 'unique-folder'));
    key('down');
    key('enter');
    await node('Folder path', field => field?.value === unique);
    key('enter');
    await shot('03-completion', shot => observed(shot, 'sentinel.txt'));
    await go(project, project);
    await shot('04-before-folder-error', shot => observed(shot, 'gone'));
    await rename(path.join(project, 'gone'), path.join(project, 'moved'));
    try {
      await clickText('gone');
      await shot('05-folder-error', shot => observed(shot, 'Retry folder'));
    } finally { await rename(path.join(project, 'moved'), path.join(project, 'gone')); }
    await clickText('Retry folder');
    await shot('06-folder-retry', shot => observed(shot, 'Empty folder'));

    const notes = path.join(project, 'notes.txt');
    await go(notes, project);
    await editor(original);
    key('e', 'cs');
    await node('Folder path', value => !value);
    const panel = (await node('File sidebar')).rect;
    // Copy path is the fifth compact toolbar button from the right in local text mode.
    click({ x: panel.x + panel.width - 124, y: panel.y + 32 + 17 });
    await eventually('copied local path', () => action('clipboard').value, value => value === notes);
    key('e', 'cs');
    clickNode(await editor(original));
    key('up', 'c');
    key('right', 'cs');
    key('c', 'c');
    await eventually('copied the first line through native movement and selection', () => action('clipboard').value, value => value === 'P4 editor first line');
    key('a', 'c');
    key('c', 'c');
    await eventually('copied source', () => action('clipboard').value, value => value === original);
    key('x', 'c');
    await editor('');
    action('paste', updated);
    await editor(updated);
    key('z', 'c');
    await editor('');
    key('z', 'cs');
    await editor(updated);
    await shot('07-editor-dirty');
    key('s', 'c');
    await eventually('saved BOM-preserving bytes', () => readFile(notes, 'utf8'), data => data === `\uFEFF${updated}`);
    await shot('08-editor-saved');

    const html = path.join(project, 'demo.html');
    const guardDraft = 'P4 draft written by Save and continue\n';
    clickNode(await editor(updated));
    key('a', 'c');
    action('paste', guardDraft);
    await editor(guardDraft);
    await go(html, project);
    await shot('08-open-with-draft', shot => observed(shot, 'Save changes to notes.txt'));
    key('escape');
    await editor(guardDraft);
    assert.equal(await readFile(notes, 'utf8'), `\uFEFF${updated}`);
    await go(html, project);
    await clickText('Save and continue');
    await eventually('save and continue committed draft bytes', () => readFile(notes, 'utf8'), data => data === `\uFEFF${guardDraft}`);
    key('e', 'cs');
    const preview = await shot('09-html-preview', shot => observed(shot, marker));
    assert.ok(preview.colors.style, 'Imported CSS must render in the native capture');
    assert.ok(preview.colors.asset, 'The local SVG must render in the native capture');
    assert.ok(Math.abs(preview.colors.asset.width / preview.scale - 24) <= 2);
    assert.ok(Math.abs(preview.colors.asset.height / preview.scale - 12) <= 2);
    assert.ok(observed(preview, 'unavailable preview assets'));
    await clickText('Containment probe');
    const blocked = await shot('10-html-after-probe', shot => observed(shot, marker));
    assert.equal(blocked.windowCount, preview.windowCount);
    await checkNetwork('Rust WKWebView');
    // Maximize/restore must keep the native preview in the live panel, not leave it floating.
    await headerControl(43);
    await node('File sidebar', value => value?.rect?.width === 1200);
    await shot('10-html-maximized', shot => observed(shot, marker));
    await headerControl(43);
    await node('File sidebar', value => value?.rect?.width === pane.rect.width);
    await shot('10-html-restored', shot => observed(shot, marker));
    const toggle = await togglePreview();
    toggle.source();
    await editor(await readFile(html, 'utf8'));
    await shot('11-html-source', shot => !observed(shot, marker));
    toggle.preview();
    action('resize-content', 40, 40, 1100, 720);
    await shot('12-html-resized', shot => shot.contentSize[0] === 1100 && shot.contentSize[1] === 720 && observed(shot, marker));
    action('resize-content', 40, 40, 1200, 760);
    key('e', 'cs');
    await go(path.join(project, 'preview.md'), project);
    await shot('13-markdown', shot => observed(shot, 'P4 Markdown heading'));
    await go(path.join(project, 'remote.svg'), project);
    await shot('14-svg', shot => observed(shot, 'remote.svg'));
    await checkNetwork('Rust standalone SVG');
    await go(path.join(project, 'oversize.txt'), project);
    await shot('15-oversize-text', shot => observed(shot, 'Retry file'));
    await go(path.join(project, 'oversize.png'), project);
    await shot('16-oversize-image', shot => observed(shot, 'Retry file'));
    await writeFile(path.join(dir, 'checks.json'), JSON.stringify({ editorBytes: `\uFEFF${guardDraft}`, scale: startup.scale,
      frame: startup.frame, htmlPreview: preview, loopbackRequests: 0 }, null, 2));

    key('b', 'cs');
    await node('File sidebar', value => !value);
    peer = await startRustPeer({ cli, helper, testcontrol, env, parent: dir, viewerData: data });
    await peer.connect();
    await clickText('P4 peer A');
    key('b', 'cs');
    await node('File sidebar');
    await go(peer.a, peer.a);
    let hasDocument = false;
    const transferControl = async (inset, offset) => {
      const { rect } = await node('File sidebar');
      click({ x: rect.x + rect.width - inset, y: rect.y + offset });
    };
    const toolbarY = 32 + 17;
    await exerciseTransfers({ parent: dir, remote: peer.a, picker, shot,
      openUpload: () => hasDocument ? transferControl(43, toolbarY) : transferControl(22, 32 + 18),
      openDownload: () => transferControl(16, toolbarY),
      async selectRemote(name) {
        await go(path.join(peer.a, name), peer.a);
        await shot('20-peer-binary', capture => observed(capture, 'Binary file'));
        key('e', 'cs');
        await node('Folder path', field => !field);
        hasDocument = true;
      },
      cancelUpload: () => transferControl(17, toolbarY),
      idle: picker.dismissed,
    });
    key('e', 'cs');
    await go(path.join(peer.a, 'scope.txt'), peer.a);
    clickNode(await editor('Draft on peer A\n'));
    const draft = 'Unsaved P4 peer draft\n';
    key('a', 'c');
    action('paste', draft);
    await editor(draft);
    await clickText('P4 peer B');
    await node('Folder path');
    await go(path.join(peer.b, 'scope.txt'), peer.b);
    await editor('Original on peer B\n');
    await clickText('Local', capture => capture.lines.find(row => row.x < capture.frame.x + 220 && /^local\b/i.test(row.text)));
    await clickText('P4 peer A');
    await editor(draft);
    await peer.cycleListener(() => shot('24-peer-listener-offline'));
    await editor(draft);
    const savedPeerFile = path.join(peer.a, 'scope.txt');
    assert.equal(await readFile(savedPeerFile, 'utf8'), 'Draft on peer A\n');
    await shot('24-peer-draft-retained');
    await clickText('P4 peer B');
    const external = 'Written by the isolated peer host while the draft was hidden\n';
    await writeFile(savedPeerFile, external);
    await clickText('P4 peer A');
    await editor(draft);
    await shot('25-peer-draft-conflict', capture => capture.lines.some(line => /changed on disk|file conflict/i.test(line.text)));
    assert.equal(await readFile(savedPeerFile, 'utf8'), external);
    await headerControl(16);
    await clickText('Discard changes');
    await node('File sidebar', value => !value);
    return startup.scale;
  } catch (error) {
    failure = error;
    try { action('capture', path.join(dir, 'last-screen.png')); } catch { /* Best effort. */ }
    throw error;
  } finally {
    await peer?.close();
    if (child.exitCode === null && child.signalCode === null && child.pid) {
      try { key('q', 'c'); } catch { child.kill('SIGTERM'); }
      const stopped = await Promise.race([exited, delay(5_000, null, { ref: false })]);
      if (!stopped) child.kill('SIGTERM');
      if (!failure) assert.equal(stopped?.code, 0, 'Owned Rust desktop did not exit cleanly');
    }
  }
}
