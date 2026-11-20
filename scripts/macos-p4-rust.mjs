import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { createWriteStream } from 'node:fs';
import { readFile, rename, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { setTimeout as delay } from 'node:timers/promises';
import { fileHash, run } from './rust-package-common.mjs';
import { marker, original, updated } from './macos-p4-fixture.mjs';

export async function eventually(label, read, predicate, ms = 20_000) {
  const deadline = Date.now() + ms;
  let last;
  while (Date.now() < deadline) {
    last = await read();
    if (predicate(last)) return last;
    await delay(300);
  }
  assert.fail(`${label}: ${JSON.stringify(last)?.slice(0, 4000)}`);
}

const keyboard = { a: 0, s: 1, d: 2, z: 6, x: 7, c: 8, v: 9, b: 11, e: 14, q: 12,
  enter: 36, tab: 48, escape: 53, down: 125 };
const named = (tree, name) => tree.find(node => node.title === name || node.description === name);
const observed = (shot, value) => shot.lines.find(row => row.text.toLowerCase().includes(value.toLowerCase()));

export async function runRust({ binary, helper, project, dir, env, window, checkNetwork }) {
  const data = path.join(dir, 'host');
  const argv = ['--shell', '--data-dir', data, '--desktop-dir', path.join(dir, 'desktop'),
    '--device-config-root', path.join(dir, 'device'), '--helper', helper, '--helper-sha256', fileHash(helper)];
  const child = spawn(binary, argv, { cwd: project, env, stdio: ['ignore', 'pipe', 'pipe'] });
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
  const togglePreview = async () => {
    const { rect } = await node('File sidebar');
    // The last two 24px toolbar controls are Source and Preview.
    const folder = named(tree(), 'Folder path');
    assert.ok(!folder, 'Collapse the browser to make the document toolbar position unambiguous');
    const press = farRight => click({ x: rect.x + rect.width - farRight, y: rect.y + 32 + 17 });
    return { source: () => press(43), preview: () => press(16) };
  };
  let failure;
  try {
    await Promise.race([exited.then(({ code }) => { throw new Error(`Rust desktop exited early: ${code}`); }),
      eventually('owned Rust Aqua window', () => {
        try { return tree(); } catch { return []; }
      }, rows => rows.length > 1, 55_000)]);
    action('activate');
    action('resize', 40, 40, 1200, 760);
    const startup = await shot('01-startup');
    assert.equal(startup.frame.width, 1200);
    assert.equal(startup.frame.height, 760);
    key('b', 'cs');
    const pane = await node('File sidebar');
    assert.equal(pane.rect.width, 420);
    await go(project, project);
    await shot('02-files', shot => observed(shot, 'notes.txt'));
    key('e', 'cs');
    await node('Folder path', field => !field);
    key('e', 'cs');
    await node('Folder path');

    await fillPath(path.join(project, 'uni'));
    await shot('03-suggestion', shot => observed(shot, 'unique-folder'));
    key('down');
    key('enter');
    const unique = path.join(project, 'unique-folder');
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
    clickNode(await editor(original));
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
    await go(html, project);
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
    const toggle = await togglePreview();
    toggle.source();
    await editor(await readFile(html, 'utf8'));
    await shot('11-html-source', shot => !observed(shot, marker));
    toggle.preview();
    action('resize', 40, 40, 1100, 720);
    await shot('12-html-resized', shot => shot.frame.width === 1100 && observed(shot, marker));
    action('resize', 40, 40, 1200, 760);
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
    await writeFile(path.join(dir, 'checks.json'), JSON.stringify({ editorBytes: `\uFEFF${updated}`, scale: startup.scale,
      frame: startup.frame, htmlPreview: preview, loopbackRequests: 0 }, null, 2));
    return startup.scale;
  } catch (error) {
    failure = error;
    try { action('capture', path.join(dir, 'last-screen.png')); } catch { /* Best effort. */ }
    throw error;
  } finally {
    if (child.exitCode === null && child.signalCode === null && child.pid) {
      try { key('q', 'c'); } catch { child.kill('SIGTERM'); }
      const stopped = await Promise.race([exited, delay(5_000, null, { ref: false })]);
      if (!stopped) child.kill('SIGTERM');
      if (!failure) assert.equal(stopped?.code, 0, 'Owned Rust desktop did not exit cleanly');
    }
  }
}
