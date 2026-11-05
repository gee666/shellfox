import { expect, test as base, type ElectronApplication, type Page } from '@playwright/test';
import { randomUUID } from 'node:crypto';
import { readFile, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { pathToFileURL } from 'node:url';
import { original, populate } from '../fixtures/files';
import type { SessionDto } from '../../src/shared/contracts';
import { launch, scratch, seedTerminal, value } from '../fixtures/electron';
import { remoteApps } from '../fixtures/remote-apps';

type FilesWorkspace = { app: ElectronApplication; page: Page; directory: string; session: SessionDto };

async function selectSession(page: Page, directory: string, title = 'Files E2E') {
  const session = value(await page.evaluate(input => window.shellfox.createSession(input), { cwd: directory, title, requestId: randomUUID() }));
  await page.getByRole('button', { name: `Select session ${title}`, exact: true }).click();
  return session;
}

const test = base.extend<{ files: FilesWorkspace }>({
  files: async ({}, use, testInfo) => {
    const directory = await scratch('files');
    await populate(directory);
    const { app, page } = await launch(path.join(directory, 'data'), 'real');
    const messages: string[] = [];
    page.on('console', message => messages.push(`${message.type()}: ${message.text().slice(0, 1000)}`));
    page.on('pageerror', error => messages.push(error.message));
    try {
      const session = await selectSession(page, directory);
      await use({ app, page, directory, session });
    } finally {
      if (testInfo.status !== testInfo.expectedStatus) {
        await testInfo.attach('files-state-before-cleanup', { body: JSON.stringify({ messages,
          workspace: await page.getByRole('main').innerText({ timeout: 1000 }).catch(() => ''),
          terminal: await page.locator('.xterm-rows').innerText({ timeout: 1000 }).catch(() => ''),
          frames: await Promise.all(page.frames().map(async frame => ({ url: frame.url().slice(0, 100), text: await frame.locator('body').innerText({ timeout: 1000 }).catch(() => '') }))),
        }), contentType: 'application/json' }).catch(() => {});
        const screenshot = await page.screenshot().catch(() => null);
        if (screenshot) await testInfo.attach('files-before-cleanup', { body: screenshot, contentType: 'image/png' }).catch(() => {});
      }
      await page.evaluate(async () => {
        const snapshot = await window.shellfox.getSnapshot();
        if (snapshot.ok) for (const session of snapshot.value.sessions) for (const tab of session.tabs) {
          if (tab.generation && tab.lifecycle !== 'closed') await window.shellfox.closeTab!({ tabId: tab.id, generation: tab.generation });
        }
      }).catch(() => {});
      // A failed assertion can leave a dirty draft. Do not let its unload guard block teardown.
      await app.evaluate(({ BrowserWindow }) => {
        for (const window of BrowserWindow.getAllWindows()) window.webContents.on('will-prevent-unload', event => event.preventDefault());
      }).catch(() => {});
      await app.close();
    }
  },
});

async function openFiles(page: Page) {
  await page.getByRole('button', { name: 'Files', exact: true }).click();
  await expect(page.getByRole('complementary', { name: 'File sidebar', exact: true })).toBeVisible();
}

async function openFile(page: Page, name: string) {
  await page.getByRole('button', { name, exact: true }).click();
}

const editor = (page: Page) => page.getByRole('textbox', { name: 'File contents', exact: true });

test('browse directories with spaces, return to parent and refresh new files', async ({ files: { page, directory } }) => {
  await openFiles(page);
  const folder = page.getByRole('treeitem').filter({ has: page.getByRole('button', { name: 'nested folder', exact: true }) });
  await expect(folder).toHaveAttribute('aria-expanded', 'false');
  await openFile(page, 'nested folder');
  await expect(folder).toHaveAttribute('aria-expanded', 'true');
  await openFile(page, 'child.txt');
  await expect(editor(page)).toHaveValue('nested content\n');
  await page.getByRole('combobox', { name: 'Folder path', exact: true }).fill(path.join(directory, 'nested folder'));
  await page.getByRole('button', { name: 'Go', exact: true }).click();
  await expect(page.getByRole('button', { name: 'notes.txt', exact: true })).toHaveCount(0);
  await expect(page.getByRole('button', { name: 'child.txt', exact: true })).toBeVisible();
  await page.getByRole('button', { name: 'Parent folder', exact: true }).click();
  await expect(page.getByRole('button', { name: 'notes.txt', exact: true })).toBeVisible();
  await writeFile(path.join(directory, 'created-later.txt'), 'fresh\n');
  await page.getByRole('button', { name: 'Refresh files', exact: true }).click();
  await openFile(page, 'created-later.txt');
  await expect(editor(page)).toHaveValue('fresh\n');
});

test('edit and save to disk, then reopen the persisted content', async ({ files: { page, directory } }) => {
  await openFiles(page);
  await openFile(page, 'notes.txt');
  await expect(editor(page)).toHaveValue(original);
  const changed = 'edited from Shellfox\nUnicode: café 日本語\n';
  await editor(page).fill(changed);
  await page.getByRole('button', { name: 'Save file', exact: true }).click();
  await expect.poll(() => readFile(path.join(directory, 'notes.txt'), 'utf8')).toBe(changed);
  await expect(page.getByRole('button', { name: 'Save file', exact: true })).toBeDisabled();
  await openFile(page, 'binary.bin');
  await openFile(page, 'notes.txt');
  await expect(editor(page)).toHaveValue(changed);
});

test('markdown preview renders formatting without frontmatter or executable HTML', async ({ files: { page } }) => {
  await openFiles(page);
  await openFile(page, 'README.md');
  await expect(page.getByRole('heading', { name: 'Files preview', exact: true })).toBeVisible();
  const preview = page.getByRole('article', { name: 'Markdown preview' });
  await expect(preview.locator('strong')).toHaveText('Bold text');
  await expect(preview.locator('code')).toHaveText('inline code');
  await expect(preview).not.toContainText('Hidden metadata');
  await expect(preview.locator('script')).toHaveCount(0);
  expect(await page.locator('body').getAttribute('data-compromised')).toBeNull();
  await expect.poll(() => preview.getByRole('img', { name: 'Markdown pixel' }).evaluate(node => node instanceof HTMLImageElement && node.naturalWidth > 0)).toBe(true);
  await page.getByRole('button', { name: 'Source', exact: true }).click();
  await expect(editor(page)).toHaveValue(/title: Hidden metadata/);
  await page.getByRole('button', { name: 'Preview', exact: true }).click();
  await preview.getByRole('link', { name: 'Open child', exact: true }).click();
  await expect(editor(page)).toHaveValue('nested content\n');
});

test('image preview decodes an image and binary content never becomes an editor', async ({ files: { page } }) => {
  await openFiles(page);
  await openFile(page, 'pixel.png');
  const image = page.getByRole('img', { name: 'pixel.png', exact: true });
  await expect(image).toBeVisible();
  await expect.poll(() => image.evaluate(node => node instanceof HTMLImageElement && node.complete && node.naturalWidth > 0)).toBe(true);
  await expect(editor(page)).toHaveCount(0);
  await openFile(page, 'binary.bin');
  await expect(page.getByText(/binary.*cannot|cannot.*binary|no preview|binary file/i)).toBeVisible();
  await expect(editor(page)).toHaveCount(0);
});

test('image rotation, crop and save persist dimensions; unsaved image edits can be discarded', async ({ files: { page, directory } }) => {
  await openFiles(page);
  await openFile(page, 'pixel.png');
  await page.getByRole('button', { name: 'Edit image', exact: true }).click();
  const frame = page.frameLocator('iframe[title="Image editing canvas"]');
  const canvas = frame.locator('canvas[aria-label="Image canvas"]');
  await expect(canvas).toHaveAttribute('width', '8');
  await expect(canvas).toHaveAttribute('height', '4');
  await frame.getByRole('button', { name: 'Rotate right', exact: true }).press('Enter');
  await expect(canvas).toHaveAttribute('width', '4');
  await expect(canvas).toHaveAttribute('height', '8');
  await expect(frame.getByRole('spinbutton')).toHaveCount(0);
  const bounds = await canvas.boundingBox();
  if (!bounds) throw new Error('Image canvas is not visible.');
  await page.mouse.move(bounds.x + 1, bounds.y + 1); await page.mouse.down();
  await page.mouse.move(bounds.x + 3, bounds.y + 6, { steps: 4 }); await page.mouse.up();
  await frame.getByRole('button', { name: 'Crop', exact: true }).press('Enter');
  await expect(canvas).toHaveAttribute('width', '2');
  await expect(canvas).toHaveAttribute('height', '5');
  await frame.getByRole('button', { name: 'Save image', exact: true }).click();
  await expect.poll(async () => { const bytes = await readFile(path.join(directory, 'pixel.png')); return [bytes.readUInt32BE(16), bytes.readUInt32BE(20)]; }).toEqual([2, 5]);
  await expect(frame.getByRole('button', { name: 'Save image', exact: true })).toBeDisabled();
  await expect(canvas).toHaveAttribute('width', '2');
  await frame.getByRole('button', { name: 'Flip horizontal', exact: true }).press('Enter');
  await expect(frame.getByRole('button', { name: 'Save image', exact: true })).toBeEnabled();
  await page.getByRole('button', { name: 'Close file sidebar', exact: true }).click();
  const confirmation = page.getByRole('alertdialog', { name: 'Unsaved changes', exact: true });
  await confirmation.getByRole('button', { name: 'Cancel', exact: true }).click();
  await expect(frame.getByRole('button', { name: 'Save image', exact: true })).toBeEnabled();
  await page.getByRole('button', { name: 'Close file sidebar', exact: true }).click();
  await confirmation.getByRole('button', { name: 'Discard changes', exact: true }).click();
  await openFiles(page);
  await openFile(page, 'pixel.png');
  await page.getByRole('button', { name: 'Edit image', exact: true }).click();
  await expect(canvas).toHaveAttribute('width', '2');
  await expect(canvas).toHaveAttribute('height', '5');
  await expect(frame.getByRole('button', { name: 'Save image', exact: true })).toBeDisabled();
});

test('HTML runs JavaScript and relative assets in a sandbox without privileged bridges', async ({ files: { app, page } }) => {
  await openFiles(page);
  await openFile(page, 'preview.html');
  const frame = page.frameLocator('iframe[title="HTML preview"]');
  await expect(frame.getByText('Ready', { exact: true })).toBeVisible();
  await expect(frame.locator('#result')).toHaveCSS('color', 'rgb(17, 34, 51)');
  await expect(frame.locator('#result')).toHaveCSS('border-top-width', '3px');
  await expect(frame.locator('#result')).toHaveCSS('background-image', /data:image\/png;base64,/);
  await expect.poll(() => frame.getByRole('img', { name: 'Relative image' }).evaluate(node => node instanceof HTMLImageElement && node.complete && node.naturalWidth > 0)).toBe(true);
  await expect(frame.locator('body')).toHaveAttribute('data-script-ready', 'true');
  await frame.getByRole('button', { name: 'Run script', exact: true }).click();
  await expect(frame.locator('#result')).toHaveText('Script ran');
  await expect(frame.locator('#isolation')).toHaveText(JSON.stringify({ shellfox: 'undefined', remote: 'undefined', require: 'undefined', process: 'undefined', ipc: 'undefined', parent: 'blocked' }));
  const sandbox = await page.locator('iframe[title="HTML preview"]').getAttribute('sandbox');
  expect(sandbox?.split(/\s+/)).toContain('allow-scripts');
  expect(sandbox?.split(/\s+/)).not.toContain('allow-same-origin');
  expect(app.windows()).toHaveLength(1);
});

test('external edits conflict instead of overwriting; dirty close can be cancelled or discarded', async ({ files: { page, directory } }) => {
  await openFiles(page);
  await openFile(page, 'notes.txt');
  await editor(page).fill('unsaved local edit\n');
  await writeFile(path.join(directory, 'notes.txt'), 'external edit\n');
  await page.getByRole('button', { name: 'Save file', exact: true }).click();
  await expect(page.getByRole('alert')).toContainText(/changed|conflict/i);
  expect(await readFile(path.join(directory, 'notes.txt'), 'utf8')).toBe('external edit\n');
  await expect(editor(page)).toHaveValue('unsaved local edit\n');
  await page.getByRole('button', { name: 'Close file sidebar', exact: true }).click();
  const confirmation = page.getByRole('alertdialog', { name: 'Unsaved changes', exact: true });
  await expect(confirmation).toBeVisible();
  await confirmation.getByRole('button', { name: 'Cancel', exact: true }).click();
  await expect(editor(page)).toHaveValue('unsaved local edit\n');
  await page.getByRole('button', { name: 'Close file sidebar', exact: true }).click();
  await confirmation.getByRole('button', { name: 'Discard changes', exact: true }).click();
  await expect(page.getByRole('complementary', { name: 'File sidebar', exact: true })).toBeHidden();
  await openFiles(page);
  await openFile(page, 'notes.txt');
  await expect(editor(page)).toHaveValue('external edit\n');
});

test('dirty file switching can be cancelled or saved, and the save shortcut persists edits', async ({ files: { page, directory } }) => {
  await openFiles(page);
  await openFile(page, 'notes.txt');
  await editor(page).fill('save before switching\n');
  await page.getByRole('button', { name: 'Files', exact: true }).click();
  await expect(page.getByRole('complementary', { name: 'File sidebar', exact: true })).toBeHidden();
  await page.getByRole('button', { name: 'Files', exact: true }).click();
  await expect(editor(page)).toHaveValue('save before switching\n');
  await openFile(page, 'README.md');
  const confirmation = page.getByRole('alertdialog', { name: 'Unsaved changes', exact: true });
  await confirmation.getByRole('button', { name: 'Cancel', exact: true }).click();
  await expect(editor(page)).toHaveValue('save before switching\n');
  await openFile(page, 'README.md');
  await confirmation.getByRole('button', { name: 'Save and continue', exact: true }).click();
  await expect(page.getByRole('heading', { name: 'Files preview', exact: true })).toBeVisible();
  expect(await readFile(path.join(directory, 'notes.txt'), 'utf8')).toBe('save before switching\n');
  await openFile(page, 'notes.txt');
  await editor(page).fill('shortcut saved\n');
  await editor(page).press(process.platform === 'darwin' ? 'Meta+s' : 'Control+s');
  await expect.poll(() => readFile(path.join(directory, 'notes.txt'), 'utf8')).toBe('shortcut saved\n');
});

test('file API rejects missing files, invalid identities and preview asset escapes', async ({ files: { page, session, directory } }) => {
  const results = await page.evaluate(async sessionId => {
    const files = window.shellfox.files;
    if (!files) throw new Error('File API is missing from preload.');
    const target = { sessionId, path: './assets/inside.html' };
    return Promise.all([
      files.read({ sessionId, path: './does-not-exist.txt' }),
      files.read({ sessionId: 'invalid', path: './notes.txt' }),
      files.read({ sessionId, tabId: crypto.randomUUID(), path: './notes.txt' }),
      files.readAsset({ ...target, assetPath: '../notes.txt' }),
      files.readAsset({ ...target, assetPath: 'https://example.com/asset.js' }),
    ]);
  }, session.id);
  for (const result of results) expect(result.ok, JSON.stringify(result)).toBe(false);
  await openFiles(page);
  await page.getByRole('combobox', { name: 'Folder path', exact: true }).fill(path.join(directory, 'does-not-exist'));
  await page.getByRole('button', { name: 'Go', exact: true }).click();
  await expect(page.getByRole('region', { name: 'File preview and editor', exact: true }).getByRole('alert')).toBeVisible();
  await page.getByRole('combobox', { name: 'Folder path', exact: true }).fill(directory);
  await page.getByRole('button', { name: 'Go', exact: true }).click();
  await openFile(page, 'notes.txt');
  await expect(editor(page)).toHaveValue(original);
});

async function clickTerminalText(page: Page, text: string, target = text) {
  const row = page.locator('.xterm-rows > div').filter({ hasText: text }).last();
  await expect(row).toBeVisible();
  // Fit/reflow follows sidebar resizing asynchronously. Do not click stale row coordinates.
  await expect.poll(async () => {
    const [rowBox, screenBox] = await Promise.all([row.boundingBox(), page.locator('.xterm-screen').boundingBox()]);
    return rowBox && screenBox ? Math.abs(rowBox.width - screenBox.width) : Infinity;
  }).toBeLessThan(1);
  const link = row.getByText(text, { exact: true });
  // xterm deliberately routes pointer events through its screen above the text spans.
  await expect(async () => {
    await link.hover({ force: true });
    await expect(page.locator('.terminal-surface')).toHaveAttribute('title', target, { timeout: 500 });
  }).toPass({ timeout: 10_000 });
  await link.click({ force: true });
}

test('plain paths and OSC file links open the sidebar while a TUI captures the mouse', async ({ files: { app, page, directory, session } }) => {
  await app.evaluate(({ shell }) => {
    const opened: string[] = [];
    Reflect.set(globalThis, '__filesOpenedExternal', opened);
    shell.openExternal = async url => { opened.push(url); };
  });
  const tab = session.tabs[0];
  if (!tab?.generation) throw new Error('No live terminal for file link test.');
  await expect(page.locator('.xterm-helper-textarea')).toBeEditable();
  const url = pathToFileURL(path.join(directory, 'notes.txt')).href + '#L2C3';
  const output = `\x1b[?1000h\x1b[?1006h\x1b[2J\x1b[H./notes.txt:2:3\r\n\x1b]8;;${url}\x1b\\osc-file-label\x1b]8;;\x1b\\\r\n`;
  const script = `process.stdin.setRawMode(true);process.stdin.resume();process.stdout.write(Buffer.from('${Buffer.from(output).toString('base64')}','base64'));process.stdin.on('data',b=>{if(b.includes(Buffer.from([27,91,60])))process.stdout.write('MOUSE_REPORT_RECEIVED')});setTimeout(()=>process.exit(0),60000)`;
  const executable = process.execPath.replaceAll("'", process.platform === 'win32' ? "''" : "'\\''");
  const command = `${process.platform === 'win32' ? '& ' : ''}'${executable}' -e "${script}"\r`;
  value(await seedTerminal(app, { tabId: tab.id, generation: tab.generation, data: command }));
  await expect(page.locator('.xterm-rows')).toContainText('osc-file-label');
  await clickTerminalText(page, './notes.txt:2:3');
  await expect(editor(page)).toHaveValue(original);
  await expect.poll(() => editor(page).evaluate(node => node instanceof HTMLTextAreaElement ? node.selectionStart : -1)).toBe('first line\n'.length + 2);
  await page.getByRole('button', { name: 'Close file sidebar', exact: true }).click();
  await clickTerminalText(page, 'osc-file-label', url);
  await expect(editor(page)).toHaveValue(original);
  expect(await app.evaluate(() => Reflect.get(globalThis, '__filesOpenedExternal'))).toEqual([]);
  await expect(page.locator('.xterm-rows')).not.toContainText('MOUSE_REPORT_RECEIVED');
  expect(app.windows()).toHaveLength(1);
});

test('remote sidebar reads, saves and loads preview assets from the approved host filesystem', async () => {
  test.setTimeout(180_000);
  const remote = await remoteApps('remote-files');
  try {
    const directory = path.join(remote.directory, 'host-work');
    await populate(directory);
    const host = await remote.open('host');
    const client = await remote.open('client');
    value(await host.page.evaluate(() => window.shellfoxRemote!.configure({ enabled: true, hostname: 'files-host' })));
    value(await client.page.evaluate(() => window.shellfoxRemote!.configure({ enabled: true, hostname: 'files-client' })));
    await expect.poll(async () => (await remote.state(client.page)).peers.some(peer => peer.id === 'files-host')).toBe(true);
    value(await client.page.evaluate(() => window.shellfoxRemote!.connect({ peerId: 'files-host' })));
    await expect.poll(async () => (await remote.state(host.page)).pending.map(peer => peer.id)).toContain('files-client');
    value(await host.page.evaluate(() => window.shellfoxRemote!.approve({ peerId: 'files-client' })));
    await expect.poll(async () => (await remote.state(client.page)).peers.find(peer => peer.id === 'files-host')?.connection).toBe('connected');
    await selectSession(host.page, directory, 'Remote files');
    await client.page.getByRole('button', { name: 'Select session Remote files', exact: true }).click();
    await openFiles(client.page);
    await openFile(client.page, 'notes.txt');
    await expect(editor(client.page)).toHaveValue(original);
    await editor(client.page).fill('saved through remote host\n');
    await client.page.getByRole('button', { name: 'Save file', exact: true }).click();
    await expect.poll(() => readFile(path.join(directory, 'notes.txt'), 'utf8')).toBe('saved through remote host\n');
    await openFile(client.page, 'preview.html');
    const preview = client.page.frameLocator('iframe[title="HTML preview"]');
    await expect(preview.locator('#result')).toHaveCSS('color', 'rgb(17, 34, 51)');
    await expect(preview.locator('body')).toHaveAttribute('data-script-ready', 'true');
    await preview.getByRole('button', { name: 'Run script', exact: true }).press('Enter');
    await expect(preview.locator('#result')).toHaveText('Script ran');
    expect(value(await client.page.evaluate(() => window.shellfox.getSnapshot())).sessions).toHaveLength(0);
  } finally { await remote.close(); }
});
