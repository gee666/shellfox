import { expect, test, type ElectronApplication, type Page } from '@playwright/test';
import { randomUUID, createHash } from 'node:crypto';
import { createReadStream } from 'node:fs';
import { mkdir, open, readFile, readdir, stat, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { remoteApps } from '../fixtures/remote-apps';
import { launch, scratch, value } from '../fixtures/electron';
import { FILE_CHUNK_BYTES } from '../../src/shared/file-transfer-schemas';

async function connected(label: string) {
  const remote = await remoteApps(label);
  try {
    const hostDirectory = path.join(remote.directory, 'host-work'), localDirectory = path.join(remote.directory, 'local-work');
    await mkdir(hostDirectory); await mkdir(localDirectory);
    const host = await remote.open('host'), client = await remote.open('client');
    value(await host.page.evaluate(() => window.shellfoxRemote!.configure({ enabled: true, hostname: 'transfer-host' })));
    value(await client.page.evaluate(() => window.shellfoxRemote!.configure({ enabled: true, hostname: 'transfer-client' })));
    await expect.poll(async () => (await remote.state(client.page)).peers.some(peer => peer.id === 'transfer-host')).toBe(true);
    value(await client.page.evaluate(() => window.shellfoxRemote!.connect({ peerId: 'transfer-host' })));
    await expect.poll(async () => (await remote.state(host.page)).pending.map(peer => peer.id)).toContain('transfer-client');
    value(await host.page.evaluate(() => window.shellfoxRemote!.approve({ peerId: 'transfer-client' })));
    await expect.poll(async () => (await remote.state(client.page)).peers.find(peer => peer.id === 'transfer-host')?.connection).toBe('connected');
    value(await host.page.evaluate(input => window.shellfox.createSession(input), { cwd: hostDirectory, title: 'Transfer host', requestId: randomUUID() }));
    await client.page.getByRole('button', { name: 'Select session Transfer host', exact: true }).click();
    await client.page.getByRole('button', { name: 'Files', exact: true }).click();
    await expect(client.page.getByRole('combobox', { name: 'Folder path', exact: true })).toHaveValue(hostDirectory);
    return { remote, host, client, hostDirectory, localDirectory };
  } catch (error) { await remote.close(); throw error; }
}
async function dialogs(app: ElectronApplication, upload: string | null, download: string | null) {
  await app.evaluate(({ dialog }, input) => {
    dialog.showOpenDialog = async () => ({ canceled: input.upload === null, filePaths: input.upload ? [input.upload] : [] });
    dialog.showSaveDialog = async () => ({ canceled: input.download === null, filePath: input.download ?? '' });
  }, { upload, download });
}
async function selectFile(page: Page, name: string) {
  await page.getByRole('button', { name, exact: true }).click();
  await expect(page.getByRole('button', { name: 'Download selected file', exact: true })).toBeEnabled();
}
async function hash(file: string) {
  const result = createHash('sha256'); for await (const chunk of createReadStream(file)) result.update(chunk); return result.digest('hex');
}

test('native file handles are revoked on reload and staged downloads are cleaned before quit', async () => {
  const directory = await scratch('native-file-owner'), source = path.join(directory, 'source.bin'), destination = path.join(directory, 'download.bin');
  await writeFile(source, Buffer.alloc(FILE_CHUNK_BYTES + 1));
  const { app, page } = await launch(path.join(directory, 'data')); let closed = false;
  try {
    await dialogs(app, source, destination);
    const picked = value(await page.evaluate(() => window.shellfox.desktopFiles!.pickUpload()));
    const sink = value(await page.evaluate(() => window.shellfox.desktopFiles!.beginDownload({ name: 'download.bin', size: 2 })));
    if (!picked || !sink) throw new Error('Native file handle was not created');
    expect(JSON.stringify(picked)).not.toContain(directory); expect(JSON.stringify(sink)).not.toContain(directory);
    value(await page.evaluate(input => window.shellfox.desktopFiles!.writeDownload(input), { ...sink, offset: 0, data: 'YQ==' }));
    await page.reload(); await page.waitForFunction(() => !!window.shellfox);
    expect(await page.evaluate(input => window.shellfox.desktopFiles!.readUpload(input), { handle: picked.handle, offset: 0 })).toMatchObject({ ok: false, error: { code: 'NOT_FOUND' } });
    expect(await page.evaluate(input => window.shellfox.desktopFiles!.writeDownload(input), { ...sink, offset: 1, data: 'Yg==' })).toMatchObject({ ok: false, error: { code: 'NOT_FOUND' } });
    await expect.poll(async () => (await readdir(directory)).filter(name => name.startsWith('.shellfox-download-'))).toEqual([]);
    expect(await readFile(destination).catch(() => null)).toBeNull();
    value(await page.evaluate(() => window.shellfox.desktopFiles!.beginDownload({ name: 'download.bin', size: 0 })));
    await app.close(); closed = true;
    expect((await readdir(directory)).filter(name => name.startsWith('.shellfox-download-'))).toEqual([]);
    expect(await readFile(destination).catch(() => null)).toBeNull();
  } finally { if (!closed) await app.close(); }
});

test('remote upload/download use native local dialogs, stream chunks, preserve existing files and handle empty files', async () => {
  test.setTimeout(240_000);
  const f = await connected('desktop-transfers');
  try {
    const payload = Buffer.alloc(FILE_CHUNK_BYTES * 2 + 17, 73);
    payload[0] = 0; payload[payload.length - 1] = 255;
    const source = path.join(f.localDirectory, 'payload.bin'), destination = path.join(f.localDirectory, 'download.bin');
    await writeFile(source, payload); await dialogs(f.client.app, source, destination);
    await expect(f.client.page.getByRole('button', { name: 'Download selected file', exact: true })).toBeDisabled();
    await f.client.page.getByRole('button', { name: 'Upload file', exact: true }).click();
    await expect(f.client.page.getByRole('status').filter({ hasText: 'Uploaded payload.bin' })).toBeVisible({ timeout: 30_000 });
    expect(await readFile(path.join(f.hostDirectory, 'payload.bin'))).toEqual(payload);
    await selectFile(f.client.page, 'payload.bin');
    await f.client.page.getByRole('button', { name: 'Download selected file', exact: true }).click();
    await expect(f.client.page.getByRole('status').filter({ hasText: 'Downloaded payload.bin' })).toBeVisible({ timeout: 30_000 });
    expect(await readFile(destination)).toEqual(payload);
    // A second upload with the same chosen name must not overwrite the host file.
    await writeFile(source, 'different local data');
    await f.client.page.getByRole('button', { name: 'Upload file', exact: true }).click();
    await expect(f.client.page.getByRole('alert').filter({ hasText: /exist|conflict/i })).toBeVisible();
    expect(await readFile(path.join(f.hostDirectory, 'payload.bin'))).toEqual(payload);
    const empty = path.join(f.localDirectory, 'empty.dat'), savedEmpty = path.join(f.localDirectory, 'empty-downloaded.dat');
    await writeFile(empty, ''); await dialogs(f.client.app, empty, savedEmpty);
    await f.client.page.getByRole('button', { name: 'Upload file', exact: true }).click();
    await expect(f.client.page.getByRole('status').filter({ hasText: 'Uploaded empty.dat' })).toBeVisible({ timeout: 30_000 });
    expect((await stat(path.join(f.hostDirectory, 'empty.dat'))).size).toBe(0);
    await selectFile(f.client.page, 'empty.dat');
    await f.client.page.getByRole('button', { name: 'Download selected file', exact: true }).click();
    await expect(f.client.page.getByRole('status').filter({ hasText: 'Downloaded empty.dat' })).toBeVisible({ timeout: 30_000 });
    expect((await stat(savedEmpty)).size).toBe(0);
    await dialogs(f.client.app, null, null);
    await f.client.page.getByRole('button', { name: 'Upload file', exact: true }).click();
    await expect(f.client.page.getByRole('button', { name: 'Upload file', exact: true })).toBeEnabled();
    await f.client.page.getByRole('button', { name: 'Download selected file', exact: true }).click();
    await expect(f.client.page.getByRole('button', { name: 'Upload file', exact: true })).toBeEnabled();
    expect((await readdir(f.hostDirectory)).sort()).toEqual(['empty.dat', 'payload.bin']);
    expect((await readdir(f.localDirectory)).filter(name => name.startsWith('.shellfox-download-'))).toEqual([]);
    const localSnapshot = value(await f.client.page.evaluate(() => window.shellfox.getSnapshot()));
    expect(localSnapshot.sessions).toHaveLength(0);
  } finally { await f.remote.close(); }
});

test('a selected binary larger than the preview cap still downloads through bounded native chunks', async () => {
  test.setTimeout(300_000);
  const f = await connected('desktop-large-download');
  try {
    const source = path.join(f.hostDirectory, 'large.bin'), destination = path.join(f.localDirectory, 'large-downloaded.bin');
    const file = await open(source, 'wx');
    try { await file.truncate(65 * 1024 * 1024); } finally { await file.close(); }
    await dialogs(f.client.app, null, destination);
    await f.client.page.getByRole('button', { name: 'Refresh files', exact: true }).click();
    await selectFile(f.client.page, 'large.bin');
    await expect(f.client.page.getByRole('region', { name: 'File preview and editor', exact: true })).toContainText('Binary file. No preview available.', { timeout: 30_000 });
    await f.client.page.getByRole('button', { name: 'Download selected file', exact: true }).click();
    await expect(f.client.page.getByRole('status').filter({ hasText: 'Downloaded large.bin' })).toBeVisible({ timeout: 180_000 });
    expect((await stat(destination)).size).toBe(65 * 1024 * 1024);
    expect(await hash(destination)).toBe(await hash(source));
    expect((await readdir(f.localDirectory)).filter(name => name.startsWith('.shellfox-download-'))).toEqual([]);
  } finally { await f.remote.close(); }
});
