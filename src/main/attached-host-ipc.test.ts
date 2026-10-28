import { afterEach, expect, it, vi } from 'vitest';
import { mkdir, mkdtemp, rm } from 'node:fs/promises';
import path from 'node:path';
import { randomUUID } from 'node:crypto';
import { BrowserWindow } from 'electron';
import { startHostRpc } from '../host/rpc';
import { connectHostClient, type HostClient } from '../host/client';
import { embeddedFixture } from './remote/test-fixtures';
import { responseSchemas } from '../shared/schemas';
import type { TerminalEvent } from '../shared/contracts';
import { installAttachedHostIpc } from './attached-host-ipc';
const url = 'file:///attached-renderer.html';
const electron = vi.hoisted(() => ({ lifecycle: new Map<string, () => void>(), handlers: new Map<string, (event: unknown, raw: unknown) => Promise<unknown>>(), send: vi.fn(), copy: vi.fn(), picker: vi.fn() }));
vi.mock('electron', () => ({
  ipcMain: { handle: (channel: string, handler: (event: unknown, raw: unknown) => Promise<unknown>) => electron.handlers.set(channel, handler), removeHandler: (channel: string) => electron.handlers.delete(channel) },
  BrowserWindow: class { isDestroyed() { return false; } webContents = { mainFrame: { url: 'file:///attached-renderer.html' }, send: electron.send, on: (event: string, listener: () => void) => { electron.lifecycle.set(event, listener); }, removeListener: (event: string, listener: () => void) => { if (electron.lifecycle.get(event) === listener) electron.lifecycle.delete(event); } }; },
  clipboard: { writeText: electron.copy, readText: async () => 'client clipboard' }, dialog: { showOpenDialog: electron.picker }, shell: { openExternal: vi.fn(), openPath: vi.fn() },
}));
const cleanups: Array<() => Promise<void>> = [];
afterEach(async () => { for (const cleanup of cleanups.splice(0).reverse()) await cleanup(); electron.handlers.clear(); electron.lifecycle.clear(); vi.restoreAllMocks(); vi.clearAllMocks(); });
async function fixture() {
  await mkdir('tmp', { recursive: true }); const directory = await mkdtemp(path.resolve('tmp/attached-ipc-'));
  const service = await embeddedFixture(), stopped = vi.fn(async () => {});
  const rpc = await startHostRpc({ userData: directory, service: service.service, mode: 'headless', shutdown: stopped });
  const client = await connectHostClient(directory), window = new BrowserWindow(), ready = vi.fn();
  const uninstall = installAttachedHostIpc(window, url, client, ready);
  cleanups.push(async () => { await uninstall(); await client.close(); await rpc.close(); await service.service.dispose(); await rm(directory, { recursive: true, force: true }); });
  const event = { sender: window.webContents, senderFrame: window.webContents.mainFrame };
  const invoke = (method: string, payload: unknown = {}, sender: unknown = event) => electron.handlers.get('manager:request')!(sender, { version: 1, method, payload });
  return { ...service, client, window, rpc, ready, stopped, uninstall, invoke };
}
it('validates renderer identity and only forwards manager contracts, never host lifecycle methods', async () => {
  const f = await fixture(), fork = vi.spyOn(f.client, 'fork'), create = vi.spyOn(f.service, 'createSession');
  expect(await f.invoke('createSession', { cwd: '/work', requestId: randomUUID() }, { sender: {}, senderFrame: f.window.webContents.mainFrame })).toMatchObject({ ok: false, error: { code: 'AUTH_FAILED' } });
  expect(fork).not.toHaveBeenCalled(); expect(create).not.toHaveBeenCalled();
  expect(await f.invoke('getSnapshot', {}, { sender: {}, senderFrame: f.window.webContents.mainFrame })).toMatchObject({ ok: false, error: { code: 'AUTH_FAILED' } });
  expect(await f.invoke('host.stop')).toMatchObject({ ok: false, error: { code: 'VALIDATION' } });
  expect(await f.invoke('createSession', { cwd: '/work', requestId: 'invalid' })).toMatchObject({ ok: false, error: { code: 'VALIDATION' } });
  for (const method of ['beginTerminalUpload', 'writeTerminalUpload', 'finishTerminalUpload', 'cancelTerminalUpload']) expect(await f.invoke(method)).toMatchObject({ ok: false, error: { code: 'VALIDATION' } });
  expect(f.stopped).not.toHaveBeenCalled(); expect(f.service.ownedTerminalCount()).toBe(0);
});
it('validates remote-only upload responses through the renderer document proxy', async () => {
  const f = await fixture(), document = await f.client.fork();
  vi.spyOn(f.client, 'fork').mockResolvedValue(document);
  const input = { peerId: 'host', method: 'finishTerminalUpload', payload: { tabId: randomUUID(), generation: randomUUID(), controlEpoch: 1, uploadId: randomUUID() } };
  const remote = () => electron.handlers.get('remote:request')!({ sender: f.window.webContents, senderFrame: f.window.webContents.mainFrame }, { method: 'request', payload: input });
  const request = vi.spyOn(document, 'request').mockResolvedValue({ ok: true, value: { path: '/host/uploads/a', fileName: 'a', size: 0 } });
  expect(await remote()).toEqual({ ok: true, value: { path: '/host/uploads/a', fileName: 'a', size: 0 } });
  expect(request).toHaveBeenCalledWith('remote.request', input);
  request.mockResolvedValue({ ok: true, value: { path: '../bad', fileName: 'a', size: 0 } });
  expect(await remote()).toMatchObject({ ok: false });
  request.mockRestore();
});
it('uses the shared host but keeps clipboard and dialogs in Electron', async () => {
  const f = await fixture();
  const created = responseSchemas.createSession.parse(await f.invoke('createSession', { cwd: '/work', requestId: randomUUID() }));
  if (!created.ok) throw new Error(created.error.message);
  const snapshot = responseSchemas.getSnapshot.parse(await f.invoke('getSnapshot'));
  expect(snapshot.ok).toBe(true); expect(f.ready).toHaveBeenCalledTimes(1);
  const tab = created.value.tabs[0];
  // Creation must use the renderer document, not the desktop metadata client.
  expect(await f.client.service.attachTerminal({ tabId: tab.id })).toMatchObject({ ok: true, value: { control: 'blocked' } });
  const attachment = responseSchemas.attachTerminal.parse(await f.invoke('attachTerminal', { tabId: tab.id }));
  if (!attachment.ok || attachment.value.control !== 'owned') throw new Error('No owner');
  await f.invoke('acknowledgeTerminal', { tabId: tab.id, generation: tab.generation, controlEpoch: attachment.value.controlEpoch, sequence: attachment.value.lastSequence });
  f.processes[0].output('shared host output');
  await vi.waitFor(() => expect(electron.send).toHaveBeenCalledWith('manager:terminal', expect.objectContaining({ type: 'data', data: 'shared host output', controlEpoch: attachment.value.controlEpoch })));
  expect(await f.invoke('copyText', { text: 'from terminal' })).toEqual({ ok: true, value: { copied: true } });
  expect(electron.copy).toHaveBeenCalledWith('from terminal');
  expect(await f.client.request('copyText', { text: 'headless cannot copy' })).toMatchObject({ ok: false, error: { code: 'UNSUPPORTED' } });
  expect(await f.invoke('readClipboardText')).toEqual({ ok: true, value: { text: 'client clipboard' } });
  await f.uninstall(); expect(f.processes[0].kill).not.toHaveBeenCalled(); expect(f.service.ownedTerminalCount()).toBe(1);
});
for (const lifecycle of ['destroyed', 'did-start-navigation', 'render-process-gone', 'uninstall']) it(`releases the document's exclusive view on ${lifecycle} without closing the shared shell`, async () => {
  const f = await fixture(), created = await f.service.createSession({ cwd: '/work', requestId: randomUUID() });
  if (!created.ok) throw new Error('No session');
  const tab = created.value.tabs[0], id = { tabId: tab.id, generation: tab.generation! };
  expect(await f.invoke('attachTerminal', id)).toMatchObject({ ok: true, value: { control: 'owned' } });
  const events: TerminalEvent[] = [];
  f.client.service.subscribeTerminal(event => events.push(event));
  expect(await f.client.service.attachTerminal(id)).toMatchObject({ ok: true, value: { control: 'blocked' } });
  if (lifecycle === 'uninstall') await f.uninstall(); else electron.lifecycle.get(lifecycle)?.();
  await vi.waitFor(() => expect(events).toContainEqual(expect.objectContaining({ type: 'control', control: 'owned', tabId: tab.id })));
  expect(await f.client.service.attachTerminal(id)).toMatchObject({ ok: true, value: { control: 'owned' } });
  expect(f.processes[0].kill).not.toHaveBeenCalled();
});
it.each(['attachTerminal', 'createSession'] as const)('does not let an old pending renderer document %s claim the next document view', async method => {
  const f = await fixture(), created = await f.service.createSession({ cwd: '/work', requestId: randomUUID() });
  if (!created.ok) throw new Error('No session');
  const id = { tabId: created.value.tabs[0].id, generation: created.value.tabs[0].generation! };
  const old = await f.client.fork(); let finish: (client: HostClient) => void = () => {};
  const pending = new Promise<HostClient>(resolve => { finish = resolve; });
  const fork = vi.spyOn(f.client, 'fork').mockReturnValueOnce(pending);
  const stale = f.invoke(method, method === 'attachTerminal' ? id : { cwd: '/new', requestId: randomUUID() });
  await vi.waitFor(() => expect(fork).toHaveBeenCalledOnce());
  electron.lifecycle.get('did-start-navigation')?.();
  expect(await f.invoke('attachTerminal', id)).toMatchObject({ ok: true, value: { control: 'owned' } });
  finish(old); expect(await stale).toMatchObject({ ok: false, error: { code: 'TARGET_LOST' } });
  await vi.waitFor(() => expect(old.request('getSnapshot')).rejects.toThrow('closed'));
  expect(await f.client.service.attachTerminal(id)).toMatchObject({ ok: true, value: { control: 'blocked' } });
  expect(f.processes[0].kill).not.toHaveBeenCalled();
  expect(f.service.ownedTerminalCount()).toBe(1);
});
