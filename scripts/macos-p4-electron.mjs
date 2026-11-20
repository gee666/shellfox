import assert from 'node:assert/strict';
import { mkdir, readFile, rename, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { _electron, expect as assertions } from '@playwright/test';
import { root } from './rust-package-common.mjs';
import { marker, original, updated } from './macos-p4-fixture.mjs';
import { eventually, nativePicker } from './macos-p4-dialog.mjs';
import { closeElectron, electronRemoteEnv, startElectronPeer } from './macos-p4-electron-peer.mjs';
import { exerciseTransfers } from './macos-p4-transfers.mjs';

const expect = assertions.configure({ timeout: 20_000 });
const dimensions = { width: 1200, height: 760 };

export async function runElectron({ binary, main, projectRoot, remoteHelper, project, dir, env, window, checkNetwork, scale, section }) {
  const data = path.join(dir, 'user-data');
  const downloads = path.join(env.HOME, 'Downloads');
  await mkdir(downloads, { mode: 0o700, recursive: true });
  const appEnv = { ...env, ...await electronRemoteEnv(dir, remoteHelper), SHELLFOX_PROJECT_ROOT: projectRoot,
    SHELLFOX_TEST_MODE: '1', SHELLFOX_TEST_VISIBLE: '1', SHELLFOX_TEST_ROOT: root,
    SHELLFOX_TEST_APP_DATA: path.join(dir, 'app-data'), SHELLFOX_TEST_LATEST_RELEASE: '' };
  const app = await _electron.launch({ executablePath: binary, cwd: root, timeout: 60_000,
    env: appEnv,
    args: [main, '--test-user-data', data, '--test-backend', 'real'],
  });
  const pid = app.process().pid;
  const action = (...args) => window(pid, binary, '--test-user-data', data, ...args);
  const picker = nativePicker(action);
  let failure, peer;
  try {
    const page = await app.firstWindow({ timeout: 60_000 });
    const owner = await app.evaluate(({ app, BrowserWindow, dialog }, { width, height, project, uploads, downloads }) => {
      app.setPath('downloads', downloads);
      const showOpen = dialog.showOpenDialog.bind(dialog);
      dialog.showOpenDialog = (parent, options) => showOpen(parent, {
        ...options,
        defaultPath: options.properties?.includes('openDirectory') ? project
          : options.title === 'Upload file' ? uploads : options.defaultPath,
      });
      const win = BrowserWindow.getAllWindows()[0];
      win.unmaximize();
      win.setPosition(40, 40);
      win.setContentSize(width, height);
      win.show();
      win.focus();
      return { pid: process.pid, executable: process.execPath, userData: app.getPath('userData') };
    }, { ...dimensions, project: path.join(project, 'unique-folder'), uploads: path.join(dir, 'upload'), downloads });
    assert.deepEqual(owner, { pid, executable: binary, userData: data });
    await expect.poll(() => page.evaluate(() => [innerWidth, innerHeight])).toEqual([1200, 760]);
    const sidebar = page.getByRole('complementary', { name: 'File sidebar' });
    const browser = page.getByRole('region', { name: 'File browser' });
    const field = browser.getByRole('combobox', { name: 'Folder path' });
    const editor = sidebar.getByRole('textbox', { name: 'File contents' });
    const button = name => sidebar.getByRole('button', { name, exact: true });
    const shot = name => action('capture', path.join(dir, `${name}.png`));
    const norm = name => path.join(project, name);
    const go = async (target, folder = target) => {
      await field.fill(target);
      await field.press('Escape');
      await button('Open path').click();
      await expect(field).toHaveValue(folder);
    };
    await expect(page.getByRole('main', { name: 'Workspace', exact: true })).toBeVisible();
    const startup = shot('01-startup');
    if (scale !== undefined) assert.equal(startup.scale, scale, 'Compare both apps on the same backing scale');
    const filesToolbar = page.getByRole('button', { name: 'Files', exact: true });
    await filesToolbar.click();
    await expect(sidebar).toBeVisible();
    if (section === 'shortcuts') {
      await go(project);
      await field.focus();
      await page.keyboard.press('Meta+Shift+b');
      await expect(sidebar).toBeHidden();
      await expect(filesToolbar).toBeFocused();
      await page.keyboard.press('Meta+Shift+b');
      await expect(field).toHaveValue(project);
      await page.keyboard.press('Meta+Shift+e');
      await expect(browser).toBeHidden();
      await expect(sidebar).toBeVisible();
      await page.keyboard.press('Meta+Shift+e');
      await expect(field).toHaveValue(project);
      shot('shortcuts');
      return;
    }
    if (section === 'workspace') {
      assert.equal((await sidebar.boundingBox()).width, 420);
      assert.equal((await sidebar.locator('.file-sidebar-header').boundingBox()).height, 32);
      assert.equal((await browser.boundingBox()).height, 220);
      assert.equal((await page.getByRole('separator', { name: 'File sidebar width' }).boundingBox()).width, 4);
      assert.equal((await page.getByRole('separator', { name: 'File browser height' }).boundingBox()).height, 4);
      const labels = await sidebar.locator('.file-sidebar-header button').evaluateAll(nodes => nodes.map(node => node.getAttribute('aria-label')));
      assert.deepEqual(labels, ['Toggle file browser', 'Open in system explorer', 'Maximize file', 'Close file sidebar']);
      await go(project);
      await button('Browse folders').click();
      const dialog = await picker.wait();
      await writeFile(path.join(dir, 'native-folder-picker.json'), JSON.stringify(dialog, null, 2));
      await picker.cancel();
      await expect(field).toHaveValue(project);
      await button('Browse folders').click();
      await picker.wait();
      await picker.confirmOpen();
      await expect(field).toHaveValue(norm('unique-folder'));
      await expect(button('sentinel.txt')).toBeVisible();
      await button('Parent folder').click();
      await expect(field).toHaveValue(project);
      await expect(button('notes.txt')).toBeVisible();
      shot('02-files');
      await field.fill(norm('uni'));
      await expect(browser.getByRole('option')).toContainText('unique-folder');
      await field.press('ArrowDown');
      await field.press('Enter');
      await expect.poll(async () => (await field.inputValue()).replace(/\/$/, '')).toBe(norm('unique-folder'));
      await field.press('Enter');
      await expect(button('sentinel.txt')).toBeVisible();
      shot('03-completion');
      await go(project);
      await expect(button('gone')).toBeVisible();
      await rename(norm('gone'), norm('moved'));
      try {
        await button('gone').click();
        await expect(button('Retry folder')).toBeVisible();
        shot('05-folder-error');
      } finally { await rename(norm('moved'), norm('gone')); }
      await button('Retry folder').click();
      await expect(browser.getByText('Empty folder', { exact: true })).toBeVisible();
      shot('06-folder-retry');

      const notes = norm('notes.txt');
      await button('notes.txt').click();
      await expect(editor).toHaveValue(`\uFEFF${original}`);
      await button('Copy file path').click();
      await expect.poll(() => action('clipboard').value).toBe(notes);
      await editor.press('Meta+ArrowUp');
      await editor.press('Meta+Shift+ArrowRight');
      await editor.press('Meta+c');
      await expect.poll(() => action('clipboard').value.replace(/^\uFEFF/, '')).toBe('P4 editor first line');
      await editor.press('Meta+a');
      await editor.press('Meta+c');
      await expect.poll(() => action('clipboard').value.replace(/^\uFEFF/, '')).toBe(original);
      await editor.press('Meta+x');
      await expect(editor).toHaveValue('');
      action('paste', updated);
      await expect(editor).toHaveValue(updated);
      await editor.press('Meta+z');
      await expect(editor).toHaveValue('');
      await editor.press('Meta+Shift+z');
      await expect(editor).toHaveValue(updated);
      // Electron exposes BOM as part of the editor's text, unlike Rust.
      await editor.fill(`\uFEFF${updated}`);
      await expect(button('Save file')).toBeEnabled();
      shot('07-editor-dirty');
      await editor.press('Meta+s');
      await expect.poll(() => readFile(notes, 'utf8')).toBe(`\uFEFF${updated}`);
      await expect(button('Save file')).toBeDisabled();
      shot('08-editor-saved');

      const guardDraft = '\uFEFFP4 draft written by Save and continue\n';
      await editor.fill(guardDraft);
      await button('demo.html').click();
      const guard = sidebar.getByRole('alertdialog');
      await expect(guard).toContainText('Save changes to notes.txt?');
      shot('08-open-with-draft');
      await page.keyboard.press('Escape');
      await expect(guard).toBeHidden();
      await expect(editor).toHaveValue(guardDraft);
      assert.equal(await readFile(notes, 'utf8'), `\uFEFF${updated}`);
      await button('demo.html').click();
      await button('Save and continue').click();
      await expect.poll(() => readFile(notes, 'utf8')).toBe(guardDraft);
      return;
    }

    if (section === 'preview') {
      await go(project);
      await button('demo.html').click();
      const preview = sidebar.frameLocator('iframe[title="HTML preview"]');
      await expect(preview.getByText(marker, { exact: true })).toHaveCSS('color', 'rgb(20, 120, 40)');
      const img = preview.getByRole('img', { name: 'Local asset' });
      await expect(img).toHaveJSProperty('naturalWidth', 24);
      await expect(img).toHaveJSProperty('naturalHeight', 12);
      await expect(sidebar.getByText('2 unavailable preview assets')).toBeVisible();
      await expect(button('Wrap lines')).toBeHidden();
      await page.keyboard.press('Meta+Shift+e');
      const screenshot = await eventually('painted Electron HTML preview', () => shot('09-html-preview'),
        image => image.lines.some(line => line.text.includes(marker)) || (image.colors.style && image.colors.asset));
      const workspaceUrl = page.url();
      await preview.getByRole('link', { name: 'Containment probe' }).click();
      await expect(page.getByTitle('HTML preview', { exact: true })).toBeVisible();
      assert.equal(page.url(), workspaceUrl);
      assert.equal(shot('10-html-after-probe').windowCount, screenshot.windowCount);
      await checkNetwork('Electron iframe');
      // Chromium can replace a CSP-denied child with a blank error document.
      // Keep this reference behavior separate from the WKWebView retention check.
      await button('Source').click();
      await button('Preview').click();
      await expect(preview.getByText(marker, { exact: true })).toBeVisible();
      await button('Maximize file').click();
      await expect(sidebar).toHaveAttribute('data-maximized', 'true');
      await expect(preview.getByText(marker, { exact: true })).toBeVisible();
      shot('10-html-maximized');
      await button('Restore file').click();
      await expect(sidebar).toHaveAttribute('data-maximized', 'false');
      await expect(preview.getByText(marker, { exact: true })).toBeVisible();
      shot('10-html-restored');
      await button('Source').click();
      await expect(button('Wrap lines')).toBeVisible();
      await expect(editor).toHaveValue(await readFile(norm('demo.html'), 'utf8'));
      shot('11-html-source');
      await button('Preview').click();
      await app.evaluate(({ BrowserWindow }) => BrowserWindow.getAllWindows()[0].setContentSize(1100, 720));
      await expect(preview.getByText(marker, { exact: true })).toBeVisible();
      shot('12-html-resized');
      await app.evaluate(({ BrowserWindow }) => BrowserWindow.getAllWindows()[0].setContentSize(1200, 760));
      await page.keyboard.press('Meta+Shift+e');
      await button('preview.md').click();
      await expect(sidebar.getByRole('heading', { name: 'P4 Markdown heading' })).toBeVisible();
      shot('13-markdown');
      await button('remote.svg').click();
      await expect(sidebar.getByRole('img', { name: 'remote.svg' })).toBeVisible();
      shot('14-svg');
      await checkNetwork('Electron standalone SVG');
      for (const [file, name] of [['oversize.txt', '15-oversize-text'], ['oversize.png', '16-oversize-image']]) {
        await button(file).click();
        await expect(sidebar.getByRole('alert')).toContainText(/large|limit/i);
        await expect(button('Retry file')).toBeVisible();
        shot(name);
      }
      await writeFile(path.join(dir, 'preview-checks.json'), JSON.stringify({ scale: startup.scale,
        frame: startup.frame, htmlPreview: screenshot, loopbackRequests: 0 }, null, 2));
      return;
    }

    assert.equal(section, 'transfers');
    await button('Close file sidebar').click();
    await expect(sidebar).toBeHidden();
    peer = await startElectronPeer({ binary, main, env: appEnv, parent: dir, client: page });
    const session = name => page.getByRole('button', { name: `Select session P4 peer ${name}`, exact: true });
    await session('A').click();
    await filesToolbar.click();
    await expect.poll(() => field.inputValue()).toBe(peer.a);
    await exerciseTransfers({ parent: dir, home: env.HOME, remote: peer.a, picker, shot,
      openUpload: () => button('Upload file').click(),
      openDownload: () => button('Download selected file').click(),
      async selectRemote(name) {
        await button('Refresh files').click();
        await button(name).click();
        await expect(button('Download selected file')).toBeEnabled();
      },
      cancelUpload: () => button('Cancel file transfer').click(),
      idle: () => expect(button('Upload file')).toBeEnabled(),
    });
    const draft = 'Unsaved P4 peer draft\n';
    await button('scope.txt').click();
    await expect(editor).toHaveValue('Draft on peer A\n');
    await editor.fill(draft);
    await session('B').click();
    await expect(field).toHaveValue(peer.b);
    await button('scope.txt').click();
    await expect(editor).toHaveValue('Original on peer B\n');
    await page.locator('[data-machine-id="local"] .session-section-toggle').click();
    await session('A').click();
    await expect(editor).toHaveValue(draft);
    await peer.cycleListener(() => shot('24-peer-listener-offline'));
    await expect(editor).toHaveValue(draft);
    const savedPeerFile = path.join(peer.a, 'scope.txt');
    assert.equal(await readFile(savedPeerFile, 'utf8'), 'Draft on peer A\n');
    shot('24-peer-draft-retained');
    await session('B').click();
    const external = 'Written by the isolated peer host while the draft was hidden\n';
    await writeFile(savedPeerFile, external);
    await session('A').click();
    await expect(editor).toHaveValue(draft);
    await expect(sidebar.getByRole('alert')).toContainText('changed on disk');
    assert.equal(await readFile(savedPeerFile, 'utf8'), external);
    shot('25-peer-draft-conflict');
    await button('Close file sidebar').click();
    await button('Discard changes').click();
    await expect(sidebar).toBeHidden();
  } catch (error) {
    failure = error;
    try { action('capture', path.join(dir, 'last-screen.png')); } catch { /* Best effort. */ }
    throw error;
  } finally {
    const cleanup = [];
    if (peer) await peer.close().catch(error => cleanup.push(error));
    await closeElectron(app).catch(error => cleanup.push(error));
    if (cleanup.length && !failure) throw new AggregateError(cleanup, 'P4 Electron cleanup failed');
  }
}
