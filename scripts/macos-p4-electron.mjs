import assert from 'node:assert/strict';
import { readFile, rename, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { setTimeout as delay } from 'node:timers/promises';
import { _electron, expect as assertions } from '@playwright/test';
import { root } from './rust-package-common.mjs';
import { marker, original, updated } from './macos-p4-fixture.mjs';

const expect = assertions.configure({ timeout: 20_000 });
const dimensions = { width: 1200, height: 760 };

export async function runElectron({ binary, main, projectRoot, project, dir, env, window, checkNetwork, scale }) {
  const data = path.join(dir, 'user-data');
  const app = await _electron.launch({ executablePath: binary, cwd: root, timeout: 60_000,
    env: { ...env, SHELLFOX_PROJECT_ROOT: projectRoot, SHELLFOX_TEST_MODE: '1', SHELLFOX_TEST_VISIBLE: '1', SHELLFOX_TEST_ROOT: root,
      SHELLFOX_TEST_APP_DATA: path.join(dir, 'app-data'), SHELLFOX_TEST_LATEST_RELEASE: '' },
    args: [main, '--test-user-data', data, '--test-backend', 'real'],
  });
  const pid = app.process().pid;
  const action = (...args) => window(pid, binary, '--test-user-data', data, ...args);
  let failure;
  try {
    const page = await app.firstWindow({ timeout: 60_000 });
    const owner = await app.evaluate(({ app, BrowserWindow }, { width, height }) => {
      const win = BrowserWindow.getAllWindows()[0];
      win.unmaximize();
      win.setContentSize(width, height);
      win.setPosition(40, 40);
      win.show();
      win.focus();
      return { pid: process.pid, executable: process.execPath, userData: app.getPath('userData') };
    }, dimensions);
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
    await page.keyboard.press('Meta+Shift+b');
    await expect(sidebar).toBeVisible();
    assert.equal((await sidebar.boundingBox()).width, 420);
    await go(project);
    await expect(button('notes.txt')).toBeVisible();
    shot('02-files');
    await page.keyboard.press('Meta+Shift+e');
    await expect(browser).toBeHidden();
    await page.keyboard.press('Meta+Shift+e');
    await expect(browser).toBeVisible();
    await field.fill(norm('uni'));
    await expect(browser.getByRole('option')).toContainText('unique-folder');
    await field.press('ArrowDown');
    await field.press('Enter');
    await expect(field).toHaveValue(norm('unique-folder'));
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
    await expect(editor).toHaveValue(original);
    await button('Copy file path').click();
    await expect.poll(() => action('clipboard').value).toBe(notes);
    await editor.press('Meta+a');
    await editor.press('Meta+c');
    await expect.poll(() => action('clipboard').value).toBe(original);
    await editor.press('Meta+x');
    await expect(editor).toHaveValue('');
    action('paste', updated);
    await expect(editor).toHaveValue(updated);
    await editor.press('Meta+z');
    await expect(editor).toHaveValue('');
    await editor.press('Meta+Shift+z');
    await expect(editor).toHaveValue(updated);
    await expect(button('Save file')).toBeEnabled();
    shot('07-editor-dirty');
    await editor.press('Meta+s');
    await expect.poll(() => readFile(notes, 'utf8')).toBe(`\uFEFF${updated}`);
    await expect(button('Save file')).toBeDisabled();
    shot('08-editor-saved');

    await button('demo.html').click();
    const preview = sidebar.frameLocator('iframe[title="HTML preview"]');
    await expect(preview.getByText(marker, { exact: true })).toHaveCSS('color', 'rgb(20, 120, 40)');
    const img = preview.getByRole('img', { name: 'Local asset' });
    await expect(img).toHaveJSProperty('naturalWidth', 24);
    await expect(img).toHaveJSProperty('naturalHeight', 12);
    await expect(sidebar.getByText('2 unavailable preview assets')).toBeVisible();
    await expect(button('Wrap lines')).toBeHidden();
    await page.keyboard.press('Meta+Shift+e');
    const screenshot = shot('09-html-preview');
    assert.ok(screenshot.lines.some(line => line.text.includes(marker)), 'Capture must include the HTML preview');
    await preview.getByRole('link', { name: 'Containment probe' }).click();
    await expect(preview.getByText(marker, { exact: true })).toBeVisible();
    assert.equal(shot('10-html-after-probe').windowCount, screenshot.windowCount);
    await checkNetwork('Electron iframe');
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
    await writeFile(path.join(dir, 'checks.json'), JSON.stringify({ editorBytes: `\uFEFF${updated}`, scale: startup.scale,
      frame: startup.frame, htmlPreview: screenshot, loopbackRequests: 0 }, null, 2));
  } catch (error) {
    failure = error;
    try { action('capture', path.join(dir, 'last-screen.png')); } catch { /* Best effort. */ }
    throw error;
  } finally {
    try {
      const closed = app.close().then(() => true);
      assert.ok(await Promise.race([closed, delay(10_000, false, { ref: false })]), 'Owned Electron did not quit');
    } catch (error) {
      if (app.process().exitCode === null) app.process().kill('SIGTERM');
      if (!failure) throw error;
    }
  }
}
