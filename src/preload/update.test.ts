import { beforeEach, expect, it, vi } from 'vitest';
import type { ManagerApi } from '../shared/contracts';
const bridge = vi.hoisted(() => ({ api: undefined as ManagerApi | undefined, invoke: vi.fn() }));
vi.mock('electron', () => ({
  contextBridge: { exposeInMainWorld: (_name: string, api: ManagerApi) => { bridge.api = api; } },
  ipcRenderer: { invoke: bridge.invoke, on: vi.fn(), removeListener: vi.fn() },
}));
import './index';
const status = { current: '0.2.5', latest: '0.2.6', available: true, command: 'shellfox update', url: 'https://github.com/gee666/shellfox/releases', phase: 'ready', supported: true, received: 12, total: 12, error: null };
beforeEach(() => bridge.invoke.mockReset());
it('exposes only validated update requests, without accepting executable paths or URLs', async () => {
  bridge.invoke.mockResolvedValue({ ok: true, value: status });
  await bridge.api!.downloadUpdate!();
  expect(bridge.invoke).toHaveBeenLastCalledWith('manager:request', { version: 1, method: 'downloadUpdate', payload: {} });
  await bridge.api!.installUpdate!({ confirmCloseTerminals: true });
  expect(bridge.invoke).toHaveBeenLastCalledWith('manager:request', { version: 1, method: 'installUpdate', payload: { confirmCloseTerminals: true } });
});
it('rejects missing, negative or forged installation consent before IPC', async () => {
  const install = bridge.api!.installUpdate! as (payload: unknown) => Promise<unknown>;
  for (const input of [{}, { confirmCloseTerminals: false }, { confirmCloseTerminals: true, command: 'evil' }]) expect(await install(input)).toMatchObject({ ok: false, error: { code: 'VALIDATION' } });
  expect(bridge.invoke).not.toHaveBeenCalled();
});
it('rejects malformed progress, arbitrary release URLs and unknown response fields', async () => {
  for (const patch of [{ received: -1 }, { total: 0 }, { phase: 'execute' }, { url: 'https://evil.example' }, { command: 'evil' }, { executable: 'evil' }]) {
    bridge.invoke.mockResolvedValueOnce({ ok: true, value: { ...status, ...patch } });
    expect(await bridge.api!.getUpdateStatus!()).toMatchObject({ ok: false, error: { code: 'INTERNAL' } });
  }
});
