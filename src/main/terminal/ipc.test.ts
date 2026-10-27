import { it, expect, vi } from 'vitest';
import type { BrowserWindow, IpcMainInvokeEvent } from 'electron';
const ipc = vi.hoisted(() => ({ handler: undefined as undefined | ((e: IpcMainInvokeEvent, raw: unknown) => Promise<any>), remove: vi.fn(), copy: vi.fn(async (_text: string) => {}), read: vi.fn(async () => ''), open: vi.fn(async (_path: string) => ''), windows: [] as BrowserWindow[] }));
vi.mock('../directory', () => ({ validateDirectory: async (cwd: string) => { if (cwd === '/missing') throw new Error('missing'); return cwd; } }));
vi.mock('electron', () => ({ clipboard: { writeText: ipc.copy, readText: ipc.read }, shell: { openPath: ipc.open }, BrowserWindow: { getAllWindows: () => ipc.windows }, ipcMain: { handle: (_channel: string, handler: typeof ipc.handler) => { ipc.handler = handler; }, removeHandler: ipc.remove }, dialog: { showOpenDialog: async () => ({ canceled: true, filePaths: [] }) } }));
import { randomUUID } from 'node:crypto';
import { responseSchemas } from '../../shared/schemas';
import { installIpc } from '../ipc';
import { EmbeddedSessionService } from './service';
import { PtyBackend } from './backend';
import { MemoryRepository, factoryFixture } from './test-fixtures';
const url = 'file:///app/index.html';
async function fixture(rendererReady?: () => void) {
  const f = factoryFixture(), service = new EmbeddedSessionService(new MemoryRepository(), new PtyBackend(f.options), () => ({ setWatch: () => {}, dispose: () => {} })); await service.initialize();
  const created = await service.createSession({ cwd: '/work', requestId: randomUUID() }); if (!created.ok) throw new Error('create');
  const frame = { url }, contents = { mainFrame: frame, send: vi.fn(), on: vi.fn(), removeListener: vi.fn() };
  const window = { webContents: contents, isDestroyed: () => false, isFocused: () => true } as unknown as BrowserWindow;
  ipc.windows = [window];
  const event = { sender: contents, senderFrame: frame } as unknown as IpcMainInvokeEvent;
  const stop = installIpc(window, url, service, rendererReady), request = (method: string, payload: unknown, e = event) => ipc.handler!(e, { version: 1, method, payload });
  return { ...f, service, session: created.value, frame, contents, window, event, stop, request };
}
it.each(['createSession', 'addTab', 'activateSession'] as const)('reserves %s for the creating embedded renderer before another view attaches', async method => {
  const f = await fixture(), other = f.service.createTerminalView(() => {});
  if (method === 'activateSession') f.processes[0].exit(0);
  const result = await f.request(method, method === 'createSession' ? { cwd: '/work', requestId: randomUUID() } : { sessionId: f.session.id });
  const session = responseSchemas[method].parse(result);
  if (!session.ok) throw new Error(session.error.message);
  const tab = session.value.tabs.at(-1)!;
  expect(other.attach({ tabId: tab.id })).toMatchObject({ ok: true, value: { control: 'blocked' } });
  expect(await f.request('attachTerminal', { tabId: tab.id })).toMatchObject({ ok: true, value: { control: 'owned' } });
  other.dispose(); f.stop(); await f.service.dispose();
});
it('routes validated tab rename/order requests and rejects untrusted senders', async () => {
  const f = await fixture(), first = f.session.tabs[0];
  const added = await f.request('addTab', { sessionId: f.session.id }), second = added.value.tabs[1];
  expect(await f.request('renameTab', { sessionId: f.session.id, tabId: first.id, title: 'Build' })).toMatchObject({ ok: true, value: { tabs: [expect.objectContaining({ title: 'Build' }), expect.anything()] } });
  expect(await f.request('reorderTabs', { sessionId: f.session.id, tabIds: [second.id, first.id] })).toMatchObject({ ok: true, value: { tabs: [expect.objectContaining({ id: second.id, ordinal: 0 }), expect.objectContaining({ id: first.id, ordinal: 1 })] } });
  expect(await f.request('renameTab', { sessionId: f.session.id, tabId: first.id, title: 'bad\n' })).toMatchObject({ ok: false, error: { code: 'VALIDATION' } });
  expect(await f.request('reorderTabs', { sessionId: f.session.id, tabIds: [first.id, first.id] })).toMatchObject({ ok: false, error: { code: 'VALIDATION' } });
  expect(await f.request('renameTab', { sessionId: f.session.id, tabId: first.id, title: 'foreign' }, { ...f.event, sender: {} } as IpcMainInvokeEvent)).toMatchObject({ ok: false, error: { code: 'AUTH_FAILED' } });
  expect(await f.request('reorderTabs', { sessionId: f.session.id, tabIds: [first.id, second.id] }, { ...f.event, senderFrame: { url } } as IpcMainInvokeEvent)).toMatchObject({ ok: false, error: { code: 'AUTH_FAILED' } });
  expect(f.service.repository.tab(first.id)?.title).toBe('Build');
  f.stop(); await f.service.dispose();
});
it('uses native clipboard/folder operations with validated ownership and awaits clipboard rejection', async () => {
  const f = await fixture(); ipc.copy.mockClear(); ipc.open.mockClear();
  expect(await f.request('copyText', { text: 'C:\\path with spaces' })).toEqual({ ok: true, value: { copied: true } });
  expect(ipc.copy).toHaveBeenCalledWith('C:\\path with spaces');
  ipc.copy.mockRejectedValueOnce(new Error('clipboard busy'));
  expect(await f.request('copyText', { text: 'x' })).toMatchObject({ ok: false, error: { code: 'INTERNAL' } });
  expect(await f.request('copyText', { text: 'x'.repeat(1024 * 1024 + 1) })).toMatchObject({ ok: false, error: { code: 'VALIDATION' } });
  ipc.read.mockResolvedValueOnce('echo hi\n');
  expect(await f.request('readClipboardText', {})).toEqual({ ok: true, value: { text: 'echo hi\n' } });
  ipc.read.mockResolvedValueOnce('x'.repeat(65537));
  expect(await f.request('readClipboardText', {})).toMatchObject({ ok: false, error: { code: 'VALIDATION' } });
  ipc.read.mockRejectedValueOnce(new Error('clipboard busy'));
  expect(await f.request('readClipboardText', {})).toMatchObject({ ok: false, error: { code: 'INTERNAL' } });
  expect(await f.request('readClipboardText', { extra: 1 })).toMatchObject({ ok: false, error: { code: 'VALIDATION' } });
  expect(await f.request('openSessionFolder', { sessionId: f.session.id })).toEqual({ ok: true, value: { opened: true } });
  expect(ipc.open).toHaveBeenCalledWith('/work');
  expect(await f.request('openSessionFolder', { sessionId: f.session.id, path: '/arbitrary' })).toMatchObject({ ok: false, error: { code: 'VALIDATION' } });
  expect(await f.request('openSessionFolder', { sessionId: randomUUID() })).toMatchObject({ ok: false, error: { code: 'NOT_FOUND' } });
  const saved = f.service.repository.session(f.session.id)!; saved.cwd = '/missing'; f.service.repository.saveSession(saved);
  expect(await f.request('openSessionFolder', { sessionId: f.session.id })).toMatchObject({ ok: false, error: { code: 'NOT_FOUND' } });
  expect(await f.request('copyText', { text: 'secret' }, { ...f.event, sender: {} } as IpcMainInvokeEvent)).toMatchObject({ ok: false, error: { code: 'AUTH_FAILED' } });
  f.stop(); await f.service.dispose();
});
it('waits for the first trusted snapshot before draining cold-start requests', async () => {
  const ready = vi.fn(), f = await fixture(ready);
  await f.request('getSnapshot', {}, { ...f.event, sender: {} } as IpcMainInvokeEvent);
  expect(ready).not.toHaveBeenCalled();
  await f.request('getSnapshot', {}); await f.request('getSnapshot', {});
  expect(ready).toHaveBeenCalledOnce(); f.stop(); await f.service.dispose();
});
it.each([false, true])('returns runtime snapshots and drains cold-start requests without waiting for slow integration metadata, failed refresh %s', async failed => {
  const ready = vi.fn(), f = await fixture(ready);
  let finish!: () => void, reject!: (error: Error) => void;
  const pending = new Promise<void>((resolve, no) => { finish = resolve; reject = no; });
  const refresh = vi.spyOn(f.service, 'refreshIntegrations').mockReturnValue(pending);
  let result: any;
  const snapshot = f.request('getSnapshot', {}).then(value => { result = value; });
  try {
    await new Promise<void>(resolve => setImmediate(resolve));
    expect(result).toMatchObject({ ok: true, value: { sessions: [expect.objectContaining({ id: f.session.id })] } });
    expect(ready).toHaveBeenCalledOnce();
  } finally {
    if (failed) reject(new Error('Registry unavailable')); else finish();
    await snapshot; await new Promise<void>(resolve => setImmediate(resolve));
    refresh.mockRestore(); f.stop(); await f.service.dispose();
  }
});
it('routes discovered profiles and validated terminal methods without exposing arbitrary commands', async () => {
  const f = await fixture(), tab = f.session.tabs[0];
  const attachment = await f.request('attachTerminal', { tabId: tab.id });
  const id = { tabId: tab.id, generation: tab.generation!, controlEpoch: attachment.value.controlEpoch };
  expect(await f.request('getTerminalProfiles', {})).toMatchObject({ ok: true, value: { lifetime: 'app-owned', shellSurvival: false } });
  expect(attachment).toMatchObject({ ok: true, value: { generation: tab.generation, control: 'owned' } });
  expect(await f.request('writeTerminal', { ...id, data: '\x03' })).toEqual({ ok: true, value: { written: true } }); expect(f.processes[0].write).toHaveBeenCalledWith('\x03');
  expect(await f.request('resizeTerminal', { ...id, cols: 100, rows: 40 })).toEqual({ ok: true, value: { resized: true } });
  expect(await f.request('writeTerminal', { ...id, data: 'x', command: 'arbitrary' })).toMatchObject({ ok: false, error: { code: 'VALIDATION' } });
  f.stop(); await f.service.dispose();
});
it('rejects foreign senders, subframes and stale generations before PTY access', async () => {
  const f = await fixture(), tab = f.session.tabs[0], payload = { tabId: tab.id, generation: tab.generation, controlEpoch: 1, data: 'x' };
  expect(await f.request('writeTerminal', payload, { ...f.event, sender: {} } as IpcMainInvokeEvent)).toMatchObject({ ok: false, error: { code: 'AUTH_FAILED' } });
  expect(await f.request('writeTerminal', payload, { ...f.event, senderFrame: { url } } as IpcMainInvokeEvent)).toMatchObject({ ok: false, error: { code: 'AUTH_FAILED' } });
  expect(await f.request('writeTerminal', { ...payload, generation: randomUUID() })).toMatchObject({ ok: false, error: { code: 'TARGET_LOST' } }); expect(f.processes[0].write).not.toHaveBeenCalled();
  f.stop(); await f.service.dispose();
});
it('streams only attached terminal views, validates acknowledgements and removes subscriptions', async () => {
  const f = await fixture(), tab = f.session.tabs[0], id = { tabId: tab.id, generation: tab.generation! };
  f.processes[0].output('hidden'); expect(f.contents.send).not.toHaveBeenCalledWith('manager:terminal', expect.objectContaining({ type: 'data' }));
  expect(f.contents.send).toHaveBeenCalledWith('manager:terminal', { type: 'activity', tabId: tab.id, busy: true });
  const replay = await f.request('attachTerminal', { tabId: tab.id }); await f.request('acknowledgeTerminal', { ...id, controlEpoch: replay.value.controlEpoch, sequence: replay.value.lastSequence }); f.processes[0].output('\x1b[32mlive'); expect(f.contents.send).toHaveBeenCalledWith('manager:terminal', expect.objectContaining({ type: 'data', data: '\x1b[32mlive' }));
  expect(await f.request('acknowledgeTerminal', { ...id, controlEpoch: replay.value.controlEpoch, sequence: 999 })).toMatchObject({ ok: false, error: { code: 'VALIDATION' } });
  expect(await f.request('detachTerminal', id)).toEqual({ ok: true, value: { detached: true } }); const count = f.contents.send.mock.calls.length; f.processes[0].output('detached'); expect(f.contents.send).toHaveBeenCalledTimes(count); expect(f.processes[0].kill).not.toHaveBeenCalled();
  f.stop(); await f.service.dispose(); expect(ipc.remove).toHaveBeenCalledWith('manager:request');
});
it('keeps unattached activity as metadata on the registered view and never foreign frames', async () => {
  const f = await fixture(), tab = f.session.tabs[0];
  const other = { isDestroyed: () => false, webContents: { mainFrame: { url }, send: vi.fn() } };
  const foreign = { isDestroyed: () => false, webContents: { mainFrame: { url: 'https://untrusted.invalid' }, send: vi.fn() } };
  ipc.windows.push(other as unknown as BrowserWindow, foreign as unknown as BrowserWindow);
  f.processes[0].output('working');
  const activity = { type: 'activity', tabId: tab.id, busy: true };
  expect(f.contents.send).toHaveBeenCalledWith('manager:terminal', activity);
  expect(other.webContents.send).not.toHaveBeenCalled();
  expect(foreign.webContents.send).not.toHaveBeenCalled();
  expect(other.webContents.send).not.toHaveBeenCalledWith('manager:terminal', expect.objectContaining({ type: 'data' }));
  f.processes[0].exit(0);
  expect(f.contents.send).toHaveBeenCalledWith('manager:terminal', { ...activity, busy: false });
  f.stop(); await f.service.dispose();
});
it('releases exclusive ownership on renderer destruction and promotes the next waiting client', async () => {
  const f = await fixture(), tab = f.session.tabs[0];
  if (!tab.generation) throw new Error('Missing generation');
  const id = { tabId: tab.id, generation: tab.generation };
  const attached = await f.request('attachTerminal', id);
  const events: unknown[] = [], waiter = f.service.createTerminalView(event => events.push(event));
  expect(waiter.attach(id)).toMatchObject({ ok: true, value: { control: 'blocked' } });
  expect(await f.request('takeTerminalControl', id, { ...f.event, sender: {} } as IpcMainInvokeEvent)).toMatchObject({ ok: false, error: { code: 'AUTH_FAILED' } });
  expect(await f.request('resizeTerminal', { ...id, controlEpoch: attached.value.controlEpoch, cols: 115, rows: 45 })).toMatchObject({ ok: true });
  const destroyed = f.contents.on.mock.calls.find(([event]) => event === 'destroyed')?.[1];
  if (typeof destroyed !== 'function') throw new Error('Missing teardown');
  destroyed();
  expect(events).toContainEqual(expect.objectContaining({ type: 'control', control: 'owned' }));
  expect(waiter.attach(id)).toMatchObject({ ok: true, value: { control: 'owned' } });
  expect(f.processes[0].kill).not.toHaveBeenCalled();
  waiter.dispose(); f.stop(); await f.service.dispose();
});
it('does not forward terminal data after the trusted frame navigates away', async () => {
  const f = await fixture(), tab = f.session.tabs[0]; await f.request('attachTerminal', { tabId: tab.id }); f.frame.url = 'https://untrusted.invalid'; f.processes[0].output('secret');
  expect(f.contents.send).not.toHaveBeenCalledWith('manager:terminal', expect.anything()); f.stop(); await f.service.dispose();
});
