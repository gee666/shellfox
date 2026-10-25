import { EventEmitter } from 'node:events';
import { randomUUID } from 'node:crypto';
import type { BrowserWindow, IpcMainInvokeEvent } from 'electron';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { CloseConfirmationCoordinator, bypassCloseConfirmation } from './close-confirmation';

const ipc = vi.hoisted(() => ({ handlers: new Map<string, (event: IpcMainInvokeEvent, raw: unknown) => unknown>() }));
vi.mock('electron', () => ({ ipcMain: {
  handle: (channel: string, handler: (event: IpcMainInvokeEvent, raw: unknown) => unknown) => ipc.handlers.set(channel, handler),
  removeHandler: (channel: string) => ipc.handlers.delete(channel),
} }));
const url = 'file:///shellfox/renderer/index.html';
let disposed: CloseConfirmationCoordinator[] = [];
beforeEach(() => ipc.handlers.clear());
afterEach(() => { disposed.forEach(coordinator => coordinator.dispose()); disposed = []; });
function fixture(autoApprove = false) {
  const webContents = Object.assign(new EventEmitter(), { mainFrame: { url }, isDestroyed: () => false, send: vi.fn() });
  const window = Object.assign(new EventEmitter(), { isDestroyed: vi.fn(() => false), webContents });
  const present = vi.fn();
  const coordinator = new CloseConfirmationCoordinator(window as unknown as BrowserWindow, url, { autoApprove, present });
  disposed.push(coordinator);
  const event = { sender: webContents, senderFrame: webContents.mainFrame } as unknown as IpcMainInvokeEvent;
  const invoke = (channel: 'get' | 'respond', raw: unknown, sender = event) => ipc.handlers.get(`app:confirmation:${channel}`)!(sender, raw);
  const ready = () => invoke('get', {});
  const current = () => webContents.send.mock.lastCall?.[1];
  return { coordinator, window, webContents, event, invoke, ready, current, present };
}

it.each([
  [false, { userData: 'isolated', backend: 'real' as const }, undefined, false],
  [false, { userData: 'isolated', backend: 'real' as const }, '1', false],
  [true, {}, undefined, false], [true, { userData: 'isolated' }, undefined, false],
  [true, { backend: 'real' as const }, undefined, false],
  [true, { userData: 'isolated', backend: 'fake' as const }, undefined, true],
  [true, { userData: 'isolated', backend: 'real' as const }, undefined, true],
  [true, { userData: 'isolated', backend: 'real' as const }, '1', false],
])('only bypasses a compiled, validated test launch: %s %j opt-in=%s', (compiled, launch, optIn, expected) => {
  expect(bypassCloseConfirmation(compiled, launch, optIn)).toBe(expected);
});

it('auto-approves default test shutdown without a renderer or a popup', async () => {
  const f = fixture(true);
  expect(await f.coordinator.request('quit', 2)).toBe(true);
  expect(await f.coordinator.request('update', 2)).toBe(true);
  expect(f.webContents.send).not.toHaveBeenCalled(); expect(f.present).not.toHaveBeenCalled();
});
it('fails closed until the trusted renderer has subscribed and read state', async () => {
  const f = fixture();
  expect(await f.coordinator.request('quit', 1)).toBe(false);
  expect(f.webContents.send).not.toHaveBeenCalled();
  expect(f.ready()).toEqual({ ok: true, value: null });
  f.window.isDestroyed.mockReturnValue(true);
  expect(await f.coordinator.request('quit', 1)).toBe(false);
});
it('waits for a matching approval, shares repeated requests, and rejects stale replies', async () => {
  const f = fixture(); f.ready();
  const pending = f.coordinator.request('quit', 3);
  expect(f.coordinator.request('quit', 3)).toBe(pending);
  expect(await f.coordinator.request('update', 3)).toBe(false);
  const confirmation = f.current();
  expect(confirmation).toEqual({ id: expect.any(String), kind: 'quit', terminalCount: 3 });
  expect(f.invoke('get', {})).toEqual({ ok: true, value: confirmation });
  expect(f.invoke('respond', { id: randomUUID(), approved: true })).toMatchObject({ ok: false, error: { code: 'NOT_FOUND' } });
  expect(f.invoke('respond', { id: confirmation.id, approved: true, command: 'quit' })).toMatchObject({ ok: false, error: { code: 'VALIDATION' } });
  expect(f.invoke('respond', { id: confirmation.id, approved: 'true' })).toMatchObject({ ok: false, error: { code: 'VALIDATION' } });
  expect(f.invoke('respond', { id: confirmation.id, approved: true })).toEqual({ ok: true, value: { accepted: true } });
  expect(await pending).toBe(true); expect(f.current()).toBeNull(); expect(f.present).toHaveBeenCalledOnce();
  expect(f.invoke('respond', { id: confirmation.id, approved: true })).toMatchObject({ ok: false, error: { code: 'NOT_FOUND' } });
});
it('cancels without closing shells and permits a later request with a fresh identity', async () => {
  const f = fixture(); f.ready();
  const first = f.coordinator.request('quit', 1), id = f.current().id;
  f.invoke('respond', { id, approved: false }); expect(await first).toBe(false);
  const second = f.coordinator.request('quit', 1);
  expect(f.current().id).not.toBe(id);
  f.invoke('respond', { id: f.current().id, approved: true }); expect(await second).toBe(true);
});
it('rejects other webContents, subframes and changed URLs on reads and answers', async () => {
  const f = fixture(); f.ready(); const pending = f.coordinator.request('quit', 1);
  const reply = { id: f.current().id, approved: true };
  for (const event of [
    { ...f.event, sender: {} }, { ...f.event, senderFrame: { url } }, { ...f.event, senderFrame: null },
  ]) {
    expect(f.invoke('get', {}, event as IpcMainInvokeEvent)).toMatchObject({ ok: false, error: { code: 'AUTH_FAILED' } });
    expect(f.invoke('respond', reply, event as IpcMainInvokeEvent)).toMatchObject({ ok: false, error: { code: 'AUTH_FAILED' } });
  }
  f.webContents.mainFrame.url = 'https://attacker.invalid';
  expect(f.invoke('respond', reply)).toMatchObject({ ok: false, error: { code: 'AUTH_FAILED' } });
  f.coordinator.dispose(); expect(await pending).toBe(false);
});
it.each(['render-process-gone', 'did-start-loading', 'closed'])('fails closed on %s without leaving a pending quit', async event => {
  const f = fixture(); f.ready(); const pending = f.coordinator.request('quit', 2);
  (event === 'closed' ? f.window : f.webContents).emit(event);
  expect(await pending).toBe(false); expect(await f.coordinator.request('quit', 2)).toBe(false);
});
it('cleans up pending approval and IPC handlers when disposed', async () => {
  const f = fixture(); f.ready(); const pending = f.coordinator.request('update', 2);
  f.coordinator.dispose(); f.coordinator.dispose();
  expect(await pending).toBe(false); expect(ipc.handlers.size).toBe(0);
  expect(f.window.listenerCount('closed')).toBe(0);
  expect(await f.coordinator.request('quit', 2)).toBe(false);
});
it('validates reads and terminal counts and cancels if window presentation fails', async () => {
  const f = fixture();
  expect(f.invoke('get', { approve: true })).toMatchObject({ ok: false, error: { code: 'VALIDATION' } });
  f.ready(); expect(await f.coordinator.request('quit', -1)).toBe(false);
  expect(await f.coordinator.request('quit', Number.NaN)).toBe(false);
  f.present.mockImplementation(() => { throw new Error('Window closed'); });
  expect(await f.coordinator.request('quit', 1)).toBe(false);
});
