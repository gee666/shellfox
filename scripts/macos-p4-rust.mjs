import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { createWriteStream } from 'node:fs';
import { readFile, rename, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { setTimeout as delay } from 'node:timers/promises';
import { fileHash, run } from './rust-package-common.mjs';
import { marker, original, updated } from './macos-p4-fixture.mjs';
import { eventually, nativePicker } from './macos-p4-dialog.mjs';
import { exerciseTransfers, startTransfer } from './macos-p4-transfers.mjs';
import { peerEnv, startRustPeer, viewerFlags } from './macos-p4-rust-peer.mjs';

const keyboard = { a: 0, s: 1, z: 6, x: 7, c: 8, b: 11, e: 14,
  escape: 53, right: 124, up: 126 };
const named = (tree, name) => tree.find(node => node.title === name || node.description === name ||
  (node.role === 'AXStaticText' && node.value === name));
const observed = (shot, value) => shot.lines.find(row => row.text.toLowerCase().includes(value.toLowerCase()));

export async function runRust({ binary, cli, helper, testcontrol, project, dir, env, window, checkNetwork, section }) {
  const data = path.join(dir, 'host');
  const argv = ['--shell', '--data-dir', data, '--desktop-dir', path.join(dir, 'desktop'),
    '--device-config-root', path.join(dir, 'device'), '--helper', helper, '--helper-sha256', fileHash(helper), ...viewerFlags];
  const child = spawn(binary, argv, { cwd: project, env: { ...env, ...peerEnv, SHELLFOX_P4_PICKER_ROOT: dir },
    stdio: ['ignore', 'pipe', 'pipe'] });
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
  // Retry only a control a repaint removed: a failed AXPress may have been delivered.
  const press = name => eventually(`pressed ${name}`, () => {
    if (named(tree(), name)?.enabled !== true) return false;
    try { action('press-control', name); }
    catch (error) {
      if (String(error).includes(`No ${name} control in an owned window`)) return false;
      throw error;
    }
    return true;
  }, Boolean);
  const transfer = name => startTransfer(name, press, () => picker.visible() || named(tree(), 'Cancel transfer'));
  const focusEditor = async value => {
    await editor(value);
    await eventually('File contents focused', () => {
      const field = named(tree(), 'File contents');
      if (field && !field.focused) action('focus-text', 'File contents');
      return field;
    }, field => field?.focused === true);
  };
  const shot = (name, want = () => true) => eventually(`Rust screenshot ${name}`, () => action('capture', path.join(dir, `${name}.png`)), want);
  const clickText = async (name, match = shot => observed(shot, name)) => click(match(await shot(`before-${name.replace(/[^a-zA-Z0-9-]/g, '_')}`, match)));
  const focusPath = async value => {
    await node('Folder path');
    if (value === undefined) action('focus-text', 'Folder path');
    else action('fill-text', 'Folder path', value);
    await node('Folder path', field => value === undefined ? field?.focused === true : field?.value === value);
  };
  const go = async (value, folder) => {
    await focusPath(value);
    await press('Open path');
    if (folder) await node('Folder path', field => field?.value === folder);
  };
  const browse = async () => {
    await press('Browse folders');
    return picker.wait();
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
    await press('Files');
    const pane = await node('File sidebar');
    assert.equal(pane.rect.width, 420);
    await go(project, project);
    if (section === 'shortcuts') {
      await focusPath();
      key('b', 'cs');
      await node('File sidebar', value => !value);
      await node('Files', value => value?.focused === true);
      key('b', 'cs');
      await node('Folder path', field => field?.value === project);
      key('e', 'cs');
      await node('Folder path', field => !field);
      await node('File sidebar');
      key('e', 'cs');
      await node('Folder path', field => field?.value === project);
      await shot('shortcuts');
      return startup.scale;
    }
    if (section === 'workspace') {
      await shot('02-files', shot => observed(shot, 'gone'));
      const dialog = await browse();
      await writeFile(path.join(dir, 'native-folder-picker.json'), JSON.stringify(dialog, null, 2));
      await picker.cancel();
      await node('Folder path', field => field?.value === project);
      await browse();
      const unique = path.join(project, 'unique-folder');
      await picker.confirmOpen();
      await node('Folder path', field => field?.value === unique);
      await shot('02-native-folder', shot => observed(shot, 'sentinel.txt'));
      await go(project, project);
      await focusPath(path.join(project, 'uni'));
      const suggestion = await eventually('named directory suggestion', () => tree().find(row =>
        row.role === 'AXButton' && [row.title, row.description].some(label => label?.replace(/\/$/, '') === unique)), Boolean);
      await shot('03-suggestion');
      await press(suggestion.title ?? suggestion.description);
      await node('Folder path', field => field?.value?.replace(/\/$/, '') === unique);
      await press('Open path');
      await shot('03-completion', shot => observed(shot, 'sentinel.txt'));
      await go(project, project);
      await shot('04-before-folder-error', shot => observed(shot, 'gone'));
      await rename(path.join(project, 'gone'), path.join(project, 'moved'));
      try {
        await press('gone');
        await node('Retry folder');
        await shot('05-folder-error');
      } finally { await rename(path.join(project, 'moved'), path.join(project, 'gone')); }
      await press('Retry folder');
      await shot('06-folder-retry', shot => observed(shot, 'Empty folder'));

      const notes = path.join(project, 'notes.txt');
      await go(notes, project);
      await editor(original);
      await press('Toggle file browser');
      await node('Folder path', value => !value);
      await press('Copy file path');
      await eventually('copied local path', () => action('clipboard').value, value => value === notes);
      await press('Toggle file browser');
      await focusEditor(original);
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
      await focusEditor(updated);
      key('a', 'c');
      action('paste', guardDraft);
      await editor(guardDraft);
      await go(html);
      await node('Save and continue', control => control?.enabled === true);
      await shot('08-open-with-draft');
      await press('Cancel');
      await editor(guardDraft);
      assert.equal(await readFile(notes, 'utf8'), `\uFEFF${updated}`);
      await go(html);
      await press('Save and continue');
      await eventually('save and continue committed draft bytes', () => readFile(notes, 'utf8'), data => data === `\uFEFF${guardDraft}`);
      await node('Edit source');
      return startup.scale;
    }

    if (section === 'preview') {
      const html = path.join(project, 'demo.html');
      await go(html, project);
      await press('Toggle file browser');
      await node('Folder path', field => !field);
      const preview = await shot('09-html-preview', shot => observed(shot, marker));
      assert.ok(preview.colors.style, 'Imported CSS must render in the native capture');
      assert.ok(preview.colors.asset, 'The local SVG must render in the native capture');
      assert.ok(Math.abs(preview.colors.asset.width / preview.scale - 24) <= 2);
      assert.ok(Math.abs(preview.colors.asset.height / preview.scale - 12) <= 2);
      await node('2 unavailable preview assets');
      await clickText('Containment probe');
      const blocked = await shot('10-html-after-probe', shot => observed(shot, marker));
      assert.equal(blocked.windowCount, preview.windowCount);
      await checkNetwork('Rust WKWebView');
      // Maximize/restore must keep the native preview in the live panel, not leave it floating.
      await press('Maximize file');
      await node('File sidebar', value => value?.rect?.width === 1200);
      await shot('10-html-maximized', shot => observed(shot, marker));
      await press('Restore file');
      await node('File sidebar', value => value?.rect?.width === pane.rect.width);
      await shot('10-html-restored', shot => observed(shot, marker));
      await press('Edit source');
      await editor(await readFile(html, 'utf8'));
      await shot('11-html-source', shot => !observed(shot, marker));
      await press('Preview file');
      await shot('11-html-preview-restored', shot => observed(shot, marker));
      action('resize-content', 40, 40, 1100, 720);
      await shot('12-html-resized', shot => shot.contentSize[0] === 1100 && shot.contentSize[1] === 720 && observed(shot, marker));
      action('resize-content', 40, 40, 1200, 760);
      await press('Toggle file browser');
      await go(path.join(project, 'preview.md'), project);
      await shot('13-markdown', shot => observed(shot, 'P4 Markdown heading'));
      await go(path.join(project, 'remote.svg'), project);
      await shot('14-svg', shot => shot.colors.svg);
      await checkNetwork('Rust standalone SVG');
      await go(path.join(project, 'oversize.txt'));
      await shot('15-oversize-text', shot => observed(shot, 'Retry file'));
      await go(path.join(project, 'oversize.png'));
      await shot('16-oversize-image', shot => observed(shot, 'Retry file'));
      await writeFile(path.join(dir, 'preview-checks.json'), JSON.stringify({ scale: startup.scale,
        frame: startup.frame, htmlPreview: preview, loopbackRequests: 0 }, null, 2));
      return startup.scale;
    }

    assert.equal(section, 'transfers');
    await press('Close file sidebar');
    await node('File sidebar', value => !value);
    peer = await startRustPeer({ cli, helper, testcontrol, env, parent: dir, viewerData: data });
    await peer.connect();
    await press('Select session P4 peer A');
    await press('Files');
    await go(peer.a, peer.a);
    await exerciseTransfers({ parent: dir, home: env.HOME, remote: peer.a, picker, shot,
      openUpload: () => transfer('Upload file'),
      openDownload: () => transfer('Download selected file'),
      async selectRemote(name) {
        await go(path.join(peer.a, name), peer.a);
        await shot('20-peer-binary', capture => observed(capture, 'Binary file'));
      },
      async startCancellationProbe() {
        await press('Upload file');
        await node('Cancel transfer', value => value?.enabled === true);
      },
      cancelUpload: () => press('Cancel transfer'),
      idle: () => node('Upload file', value => value?.enabled === true),
    });
    await go(path.join(peer.a, 'scope.txt'), peer.a);
    await focusEditor('Draft on peer A\n');
    const draft = 'Unsaved P4 peer draft\n';
    key('a', 'c');
    action('paste', draft);
    await editor(draft);
    await press('Select session P4 peer B');
    await go(peer.b, peer.b);
    // Ordinary tabs on one peer share Files. Browsing B must keep A's draft.
    await editor(draft);
    assert.equal(await readFile(path.join(peer.b, 'scope.txt'), 'utf8'), 'Original on peer B\n');
    await press('Local sessions');
    await press('Select session P4 peer A');
    await go(peer.a, peer.a);
    await editor(draft);
    const peerSession = 'Select session P4 peer A';
    await node(peerSession);
    await peer.cycleListener(async () => {
      await eventually('desktop observed the stopped peer listener', tree,
        rows => named(rows, 'Peer connection') && !named(rows, peerSession), 75_000);
      await shot('24-peer-listener-offline');
    });
    await node(peerSession);
    await editor(draft);
    const savedPeerFile = path.join(peer.a, 'scope.txt');
    assert.equal(await readFile(savedPeerFile, 'utf8'), 'Draft on peer A\n');
    await shot('24-peer-draft-retained');
    await press('Local sessions');
    const external = 'Written by the isolated peer host while the draft was hidden\n';
    await writeFile(savedPeerFile, external);
    await press('Select session P4 peer A');
    // A transport failure after the listener cycle may re-pair the peer while the
    // draft is hidden. Files then keeps the draft off the new connection instead.
    const retired = rows => named(rows, 'Discard old draft and switch Files');
    const returned = await eventually('draft returned to its source', tree,
      rows => retired(rows) || named(rows, 'File contents')?.value === draft);
    if (retired(returned)) {
      await shot('25-peer-draft-conflict', capture => observed(capture, 'changed while disconnected'));
      assert.equal(await readFile(savedPeerFile, 'utf8'), external);
      await press('Discard old draft and switch Files');
      await node('Folder path');
      await press('Close file sidebar');
    } else {
      const conflict = rows => rows.some(row => [row.title, row.description, row.value].some(label =>
        typeof label === 'string' && /changed on disk|file conflict/i.test(label)));
      const visible = await eventually('draft revision checked after returning to the source', tree,
        rows => conflict(rows) || named(rows, 'Save file')?.enabled);
      if (!conflict(visible)) await press('Save file');
      await eventually('conflict refused the stale draft', tree, conflict);
      await shot('25-peer-draft-conflict');
      assert.equal(await readFile(savedPeerFile, 'utf8'), external);
      await press('Close file sidebar');
      await press('Discard changes');
    }
    await node('File sidebar', value => !value);
    await press('Local sessions');
    return startup.scale;
  } catch (error) {
    failure = error;
    try { action('capture', path.join(dir, 'last-screen.png')); } catch { /* Best effort. */ }
    try {
      await writeFile(path.join(dir, 'last-tree.json'), JSON.stringify({ window: tree(), dialogs: action('dialogs') }, null, 2));
    } catch { /* Best effort. */ }
    throw error;
  } finally {
    try {
      if (child.pid && child.exitCode === null && child.signalCode === null) {
        let closeError;
        let nextClose = Date.now();
        let confirmed = false;
        const stopped = await eventually('owned Rust desktop exited', async () => {
          const result = await Promise.race([exited, delay(200, null)]);
          if (result || confirmed || Date.now() < nextClose) return result;
          nextClose = Date.now() + 2_000;
          try {
            if (named(tree(), 'Close shells and quit')?.enabled === true) {
              action('press-control', 'Close shells and quit');
              confirmed = true;
            } else {
              // Hiding a peer terminal can still leave an in-flight detach.
              // The close guard rejects the request until that receipt arrives.
              action('close-window');
            }
          } catch (error) { closeError = error; }
          return null;
        }, Boolean, 40_000).catch(error => {
          if (!failure) {
            try { action('capture', path.join(dir, 'close-screen.png')); } catch { /* Best effort. */ }
            throw closeError ?? error;
          }
        });
        if (!failure) assert.equal(stopped?.code, 0, `Owned Rust desktop exit: ${JSON.stringify(stopped)}`);
      }
    } finally {
      if (child.exitCode === null && child.signalCode === null) child.kill('SIGTERM');
      await peer?.close();
    }
  }
}
