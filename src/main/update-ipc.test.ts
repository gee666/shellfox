import { beforeEach, expect, it, vi } from 'vitest';
import type { BrowserWindow, IpcMainInvokeEvent } from 'electron';
import type { SessionService } from './service';
const state = vi.hoisted(() => ({ handler: undefined as undefined | ((event: unknown, request: unknown) => Promise<unknown>) }));
vi.mock('electron', () => ({
  ipcMain: { handle: (_channel: string, fn: typeof state.handler) => { state.handler = fn; }, removeHandler: () => {} },
  dialog: { showOpenDialog: async () => ({ canceled: true, filePaths: [] }) },
}));
import { installIpc } from './ipc';
import { UpdateChecker } from './update-check';
const url = 'file:///C:/app/index.html';
function fixture() {
  const frame = { url }, contents = { mainFrame: frame, send: vi.fn() };
  const window = { webContents: contents, isDestroyed: () => false, isFocused: () => true } as unknown as BrowserWindow;
  const event = { sender: contents, senderFrame: frame } as unknown as IpcMainInvokeEvent;
  const service = { backend: {}, subscribe: vi.fn(() => () => {}) } as unknown as SessionService;
  return { window, event, service };
}
beforeEach(() => { state.handler = undefined; });
const request = { version: 1, method: 'getUpdateStatus', payload: {} };
it('returns the cached update status to the trusted renderer', async () => {
  const { window, event, service } = fixture(); const check = vi.fn(async () => '0.2.0');
  installIpc(window, url, service, undefined, new UpdateChecker({ current: '0.1.1', check }));
  const expected = { ok: true, value: { current: '0.1.1', latest: '0.2.0', available: true, command: 'shellfox update', url: 'https://github.com/gee666/shellfox/releases' } };
  expect(await state.handler!(event, request)).toEqual(expected); expect(await state.handler!(event, request)).toEqual(expected);
  expect(check).toHaveBeenCalledTimes(1);
});
it('rejects untrusted senders and payloads, and is unsupported without a checker', async () => {
  const { window, event, service } = fixture(); const check = vi.fn(async () => '0.2.0');
  installIpc(window, url, service, undefined, new UpdateChecker({ current: '0.1.1', check }));
  expect(await state.handler!({ ...event, sender: {} }, request)).toMatchObject({ ok: false, error: { code: 'AUTH_FAILED' } });
  expect(await state.handler!(event, { ...request, payload: { x: 1 } })).toMatchObject({ ok: false, error: { code: 'VALIDATION' } });
  expect(check).not.toHaveBeenCalled();
  installIpc(window, url, service);
  expect(await state.handler!(event, request)).toMatchObject({ ok: false, error: { code: 'UNSUPPORTED' } });
});
