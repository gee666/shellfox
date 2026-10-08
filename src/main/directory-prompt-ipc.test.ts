import { beforeEach, expect, it, vi } from 'vitest';
import type { BrowserWindow, IpcMainInvokeEvent } from 'electron';
import type { ManagerApi } from '../shared/contracts';
import type { SessionService } from './service';

const bridge = vi.hoisted(() => ({
  api: undefined as ManagerApi | undefined,
  handler: undefined as undefined | ((event: unknown, request: unknown) => Promise<unknown>),
  event: undefined as unknown,
  invoke: vi.fn(), home: vi.fn(), resolve: vi.fn(), complete: vi.fn(),
}));
vi.mock('./directory-prompt', () => ({ getHomeDirectory: bridge.home, resolveDirectory: bridge.resolve, completeDirectory: bridge.complete }));
vi.mock('electron', () => ({
  ipcMain: { handle: (_channel: string, handler: typeof bridge.handler) => { bridge.handler = handler; }, removeHandler: vi.fn() },
  ipcRenderer: { invoke: bridge.invoke, on: vi.fn(), removeListener: vi.fn() },
  contextBridge: { exposeInMainWorld: (_name: string, api: ManagerApi) => { bridge.api = api; } },
}));
import { installIpc } from './ipc';
import '../preload/index';

const cwd = process.platform === 'win32' ? 'C:\\Users\\Tester' : '/home/tester';
beforeEach(() => {
  bridge.home.mockReset().mockReturnValue({ ok: true, value: { cwd } });
  bridge.resolve.mockReset().mockResolvedValue({ ok: true, value: { cwd } });
  bridge.complete.mockReset().mockResolvedValue({ ok: true, value: { matches: ['~/folder/'] } });
  bridge.invoke.mockReset().mockImplementation((_channel, request) => bridge.handler!(bridge.event, request));
  const url = 'file:///app/index.html', mainFrame = { url }, webContents = { mainFrame, send: vi.fn() };
  const window = { webContents, isDestroyed: () => false, isFocused: () => true } as unknown as BrowserWindow;
  bridge.event = { sender: webContents, senderFrame: mainFrame } as unknown as IpcMainInvokeEvent;
  installIpc(window, url, { subscribe: () => () => {} } as unknown as SessionService);
});

it('exposes required methods through the validated explicit manager IPC', async () => {
  expect(await bridge.api!.getHomeDirectory()).toEqual({ ok: true, value: { cwd } });
  expect(bridge.invoke).toHaveBeenLastCalledWith('manager:request', { version: 1, method: 'getHomeDirectory', payload: {} });
  expect(bridge.home).toHaveBeenCalledTimes(1);
  expect(await bridge.api!.resolveDirectory({ path: '"~/folder"' })).toEqual({ ok: true, value: { cwd } });
  expect(bridge.resolve).toHaveBeenCalledExactlyOnceWith({ path: '"~/folder"' });
  expect(await bridge.api!.completeDirectory({ path: '~/fo' })).toEqual({ ok: true, value: { matches: ['~/folder/'] } });
  expect(bridge.complete).toHaveBeenCalledExactlyOnceWith({ path: '~/fo' });
});

it('rejects invalid inputs in preload before invoking IPC', async () => {
  expect(await bridge.api!.resolveDirectory({ path: '\0' })).toMatchObject({ ok: false, error: { code: 'VALIDATION' } });
  expect(await bridge.api!.completeDirectory({ path: 'x'.repeat(32768) })).toMatchObject({ ok: false, error: { code: 'VALIDATION' } });
  expect(bridge.invoke).not.toHaveBeenCalled();
  expect(bridge.resolve).not.toHaveBeenCalled();
  expect(bridge.complete).not.toHaveBeenCalled();
});

it('rejects unauthorized or malformed path requests in main before invoking helpers', async () => {
  for (const method of ['getHomeDirectory', 'resolveDirectory', 'completeDirectory']) {
    const payload = method === 'getHomeDirectory' ? {} : { path: '' };
    expect(await bridge.handler!({}, { version: 1, method, payload })).toMatchObject({ ok: false, error: { code: 'AUTH_FAILED' } });
    expect(await bridge.handler!(bridge.event, { version: 1, method, payload: { ...payload, extra: true } })).toMatchObject({ ok: false, error: { code: 'VALIDATION' } });
  }
  expect(bridge.home).not.toHaveBeenCalled(); expect(bridge.resolve).not.toHaveBeenCalled(); expect(bridge.complete).not.toHaveBeenCalled();
});

it('validates helper responses and forwards clear failures unchanged', async () => {
  bridge.resolve.mockResolvedValueOnce({ ok: true, value: { cwd: 'relative', private: true } });
  expect(await bridge.api!.resolveDirectory({ path: '' })).toMatchObject({ ok: false, error: { code: 'INTERNAL' } });
  bridge.complete.mockResolvedValueOnce({ ok: true, value: { matches: Array(101).fill('dir/') } });
  expect(await bridge.api!.completeDirectory({ path: '' })).toMatchObject({ ok: false, error: { code: 'INTERNAL' } });
  const error = { ok: false, error: { code: 'NOT_FOUND', message: 'Cannot complete path: directory does not exist.', retryable: false } };
  bridge.complete.mockResolvedValueOnce(error);
  expect(await bridge.api!.completeDirectory({ path: 'missing/' })).toEqual(error);
});

it('preload independently rejects malformed IPC responses', async () => {
  bridge.invoke.mockResolvedValueOnce({ ok: true, value: { matches: ['dir/'], command: 'bad' } });
  expect(await bridge.api!.completeDirectory({ path: '' })).toMatchObject({ ok: false, error: { code: 'INTERNAL' } });
});
