import { afterEach, expect, it, vi } from 'vitest';
import { mkdir, mkdtemp, rm } from 'node:fs/promises';
import path from 'node:path';
import { randomUUID } from 'node:crypto';
import { BrowserWindow } from 'electron';
import { startHostRpc } from '../host/rpc';
import { connectHostClient } from '../host/client';
import { embeddedFixture } from './remote/test-fixtures';
import { responseSchemas } from '../shared/schemas';
import { installAttachedHostIpc } from './attached-host-ipc';
const url = 'file:///attached-renderer.html';
const electron = vi.hoisted(() => ({ handlers: new Map<string, (event: unknown, raw: unknown) => Promise<unknown>>(), send: vi.fn(), copy: vi.fn(), picker: vi.fn() }));
vi.mock('electron', () => ({
  ipcMain: { handle: (channel: string, handler: (event: unknown, raw: unknown) => Promise<unknown>) => electron.handlers.set(channel, handler), removeHandler: (channel: string) => electron.handlers.delete(channel) },
  BrowserWindow: class { isDestroyed() { return false; } webContents = { mainFrame: { url: 'file:///attached-renderer.html' }, send: electron.send }; },
  clipboard: { writeText: electron.copy, readText: async () => 'client clipboard' }, dialog: { showOpenDialog: electron.picker }, shell: { openExternal: vi.fn(), openPath: vi.fn() },
}));
const cleanups: Array<() => Promise<void>> = [];
afterEach(async () => { for (const cleanup of cleanups.splice(0).reverse()) await cleanup(); electron.handlers.clear(); vi.clearAllMocks(); });
async function fixture() {
  await mkdir('tmp', { recursive: true }); const directory = await mkdtemp(path.resolve('tmp/attached-ipc-'));
  const service = await embeddedFixture();
  const stopped = vi.fn(async () => {});
  const rpc = await startHostRpc({ userData: directory, service: service.service, mode: 'headless', shutdown: stopped });
  const client = await connectHostClient(directory), window = new BrowserWindow();
  const ready = vi.fn();
  const uninstall = installAttachedHostIpc(window, url, client, ready);
  cleanups.push(async () => { uninstall(); await client.close(); await rpc.close(); await service.service.dispose(); await rm(directory, { recursive: true, force: true }); });
  const event = { sender: window.webContents, senderFrame: window.webContents.mainFrame };
  const invoke = (method: string, payload: unknown = {}, sender: unknown = event) => electron.handlers.get('manager:request')!(sender, { version: 1, method, payload });
  return { ...service, client, window, rpc, ready, stopped, uninstall, invoke };
}
it('validates renderer identity and only forwards manager contracts, never host lifecycle methods', async () => {
  const f = await fixture();
  expect(await f.invoke('getSnapshot', {}, { sender: {}, senderFrame: f.window.webContents.mainFrame })).toMatchObject({ ok: false, error: { code: 'AUTH_FAILED' } });
  expect(await f.invoke('host.stop')).toMatchObject({ ok: false, error: { code: 'VALIDATION' } });
  expect(await f.invoke('createSession', { cwd: '/work', requestId: 'invalid' })).toMatchObject({ ok: false, error: { code: 'VALIDATION' } });
  expect(f.stopped).not.toHaveBeenCalled(); expect(f.service.ownedTerminalCount()).toBe(0);
});
it('uses the shared host sessions and stream but keeps clipboard and dialogs in Electron', async () => {
  const f = await fixture();
  const created = responseSchemas.createSession.parse(await f.invoke('createSession', { cwd: '/work', requestId: randomUUID() }));
  if (!created.ok) throw new Error(created.error.message);
  const snapshot = responseSchemas.getSnapshot.parse(await f.invoke('getSnapshot'));
  expect(snapshot.ok).toBe(true); expect(f.ready).toHaveBeenCalledTimes(1);
  const tab = created.value.tabs[0];
  const attachment = responseSchemas.attachTerminal.parse(await f.invoke('attachTerminal', { tabId: tab.id }));
  if (!attachment.ok) throw new Error(attachment.error.message);
  await f.invoke('acknowledgeTerminal', { tabId: tab.id, generation: tab.generation, sequence: attachment.value.lastSequence });
  f.processes[0].output('shared host output');
  await vi.waitFor(() => expect(electron.send).toHaveBeenCalledWith('manager:terminal', expect.objectContaining({ type: 'data', data: 'shared host output' })));
  expect(await f.invoke('copyText', { text: 'from terminal' })).toEqual({ ok: true, value: { copied: true } });
  expect(electron.copy).toHaveBeenCalledWith('from terminal');
  expect(await f.client.request('copyText', { text: 'headless cannot copy' })).toMatchObject({ ok: false, error: { code: 'UNSUPPORTED' } });
  expect(await f.invoke('readClipboardText')).toEqual({ ok: true, value: { text: 'client clipboard' } });
  f.uninstall(); await f.client.close();
  expect(f.processes[0].kill).not.toHaveBeenCalled(); expect(f.service.ownedTerminalCount()).toBe(1);
});
it('marks displayed terminals unavailable when the host connection is lost', async () => {
  const f = await fixture();
  const created = await f.service.createSession({ cwd: '/work', requestId: randomUUID() }); if (!created.ok) throw new Error('create failed');
  await f.invoke('getSnapshot');
  await f.rpc.close();
  await vi.waitFor(() => expect(electron.send).toHaveBeenCalledWith('manager:terminal', expect.objectContaining({ type: 'error', tabId: created.value.tabs[0].id, error: expect.objectContaining({ code: 'NATIVE_UNAVAILABLE' }) })));
  expect(await f.invoke('renameSession', { sessionId: created.value.id, title: 'uncertain' })).toMatchObject({ ok: false, error: { code: 'NATIVE_UNAVAILABLE' } });
});
