import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { EventEmitter } from 'node:events';
import { mkdir, mkdtemp, readdir, rm, writeFile } from 'node:fs/promises';
import path from 'node:path';
import type { BrowserWindow, IpcMainInvokeEvent } from 'electron';
import { DESKTOP_FILES_CHANNEL } from '../shared/desktop-file-schemas';
const state = vi.hoisted(() => ({
  handlers: new Map<string, (event: IpcMainInvokeEvent, raw: unknown) => Promise<unknown>>(),
  upload: vi.fn<() => Promise<{ canceled: boolean; filePaths: string[] }>>(),
  download: vi.fn<() => Promise<{ canceled: boolean; filePath?: string }>>(), directory: '',
}));
vi.mock('electron', () => ({
  app: { getPath: () => state.directory },
  dialog: { showOpenDialog: state.upload, showSaveDialog: state.download },
  ipcMain: { handle: (channel: string, handler: (event: IpcMainInvokeEvent, raw: unknown) => Promise<unknown>) => state.handlers.set(channel, handler), removeHandler: (channel: string) => state.handlers.delete(channel) },
}));
import { installDesktopFilesIpc } from './desktop-files-ipc';
const url = 'file:///app/index.html';
let dispose: () => Promise<void> = async () => {};
function fixture() {
  const frame = { url }, contents = Object.assign(new EventEmitter(), { mainFrame: frame });
  const window = { webContents: contents, isDestroyed: () => false } as unknown as BrowserWindow;
  const event = { sender: contents, senderFrame: frame } as unknown as IpcMainInvokeEvent;
  dispose = installDesktopFilesIpc(window, url);
  const request = (method: string, payload: unknown = {}, sender = event) => {
    const handler = state.handlers.get(DESKTOP_FILES_CHANNEL); if (!handler) throw new Error('Missing local file handler');
    return handler(sender, { version: 1, method, payload });
  };
  return { contents, event, request };
}
beforeEach(async () => {
  await mkdir('tmp', { recursive: true }); state.directory = await mkdtemp(path.resolve('tmp/desktop-files-ipc-'));
  state.handlers.clear(); state.upload.mockReset().mockResolvedValue({ canceled: true, filePaths: [] }); state.download.mockReset().mockResolvedValue({ canceled: true });
});
afterEach(async () => { await dispose(); vi.useRealTimers(); await rm(state.directory, { recursive: true, force: true }); });
it('requires the exact trusted owner main frame before opening a native picker', async () => {
  const f = fixture();
  expect(await f.request('pickUpload', {}, { ...f.event, sender: {} } as unknown as IpcMainInvokeEvent)).toMatchObject({ ok: false, error: { code: 'AUTH_FAILED' } });
  expect(await f.request('pickUpload', {}, { ...f.event, senderFrame: { url } } as unknown as IpcMainInvokeEvent)).toMatchObject({ ok: false, error: { code: 'AUTH_FAILED' } });
  expect(state.upload).not.toHaveBeenCalled();
  expect(await f.request('pickUpload')).toEqual({ ok: true, value: null }); expect(state.upload).toHaveBeenCalledOnce();
});
it('rejects arbitrary file paths and unknown methods on the separate local channel', async () => {
  const f = fixture();
  for (const [method, payload] of [['pickUpload', { path: '/private' }], ['readUpload', { handle: '/private', offset: 0 }], ['beginDownload', { name: '../escape', size: 0 }], ['readFile', { path: '/private' }]]) {
    expect(await f.request(String(method), payload)).toMatchObject({ ok: false, error: { code: 'VALIDATION' } });
  }
  expect(state.upload).not.toHaveBeenCalled(); expect(state.download).not.toHaveBeenCalled();
});
it('defaults the save dialog to Downloads, requests overwrite confirmation and exposes only an opaque handle', async () => {
  const target = path.join(state.directory, 'target'); state.download.mockResolvedValueOnce({ canceled: false, filePath: target });
  const f = fixture(), result = await f.request('beginDownload', { name: 'source.bin', size: 0 });
  expect(result).toMatchObject({ ok: true, value: { handle: expect.any(String) } });
  expect(JSON.stringify(result)).not.toContain(target);
  expect(state.download).toHaveBeenCalledWith(expect.anything(), expect.objectContaining({ defaultPath: path.join(state.directory, 'source.bin'), properties: expect.arrayContaining(['showOverwriteConfirmation']) }));
  f.contents.emit('did-start-navigation', {}, 'about:blank', false, true);
  await vi.waitFor(async () => expect(await readdir(state.directory)).toEqual([]));
});
it('revokes handles on main-document navigation but not on sandboxed child navigation', async () => {
  const file = path.join(state.directory, 'source'); await writeFile(file, 'contents'); state.upload.mockResolvedValue({ canceled: false, filePaths: [file] });
  const f = fixture(), selected = await f.request('pickUpload');
  const parsed = (await import('../shared/desktop-file-schemas')).desktopFileResponseSchemas.pickUpload.parse(selected);
  if (!parsed.ok || !parsed.value) throw new Error('No upload handle');
  f.contents.emit('did-start-navigation', {}, 'shellfox-preview://document/', false, false);
  expect(await f.request('readUpload', { handle: parsed.value.handle, offset: 0 })).toMatchObject({ ok: true });
  f.contents.emit('did-start-navigation', {}, url, false, true);
  expect(await f.request('readUpload', { handle: parsed.value.handle, offset: 8 })).toMatchObject({ ok: false, error: { code: 'NOT_FOUND' } });
});
it('has no transport timer while a native picker is open and rejects a picker completed after reload', async () => {
  vi.useFakeTimers(); let choose = () => {};
  state.upload.mockImplementation(() => new Promise(resolve => { choose = () => resolve({ canceled: true, filePaths: [] }); }));
  const f = fixture(); let settled = false; const request = f.request('pickUpload').then(result => { settled = true; return result; });
  await vi.advanceTimersByTimeAsync(10 * 60_000); expect(settled).toBe(false);
  f.contents.emit('did-start-navigation', {}, url, false, true); choose();
  expect(await request).toMatchObject({ ok: false, error: { code: 'AUTH_FAILED' } });
});
it('cleans staged downloads and unregisters the channel when the owner is destroyed', async () => {
  state.download.mockResolvedValueOnce({ canceled: false, filePath: path.join(state.directory, 'target') }); const f = fixture();
  await f.request('beginDownload', { name: 'target', size: 3 }); expect(await readdir(state.directory)).toHaveLength(1);
  f.contents.emit('destroyed'); await dispose();
  expect(state.handlers.has(DESKTOP_FILES_CHANNEL)).toBe(false); expect(await readdir(state.directory)).toEqual([]);
});
