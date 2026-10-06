import { beforeEach, it, expect, vi } from 'vitest';
import { randomUUID } from 'node:crypto';
import type { ManagerApi } from '../shared/contracts';
const bridge = vi.hoisted(() => ({ api: undefined as ManagerApi | undefined, listeners: new Map<string, Set<(e: unknown, value: unknown) => void>>(), invoke: vi.fn(), removed: vi.fn() }));
vi.mock('electron', () => ({ contextBridge: { exposeInMainWorld: (name: string, api: ManagerApi) => { if (name !== 'shellfox') throw new Error('Unexpected bridge'); bridge.api = api; } }, ipcRenderer: {
  invoke: bridge.invoke,
  on: (channel: string, fn: (e: unknown, value: unknown) => void) => { const set = bridge.listeners.get(channel) ?? new Set(); set.add(fn); bridge.listeners.set(channel, set); },
  removeListener: (channel: string, fn: (e: unknown, value: unknown) => void) => { bridge.listeners.get(channel)?.delete(fn); bridge.removed(channel); },
} }));
import './index';
beforeEach(() => { bridge.listeners.clear(); bridge.invoke.mockReset(); bridge.removed.mockReset(); });
it('exposes frozen typed methods and never raw IPC or node access', () => {
  expect(Object.isFrozen(bridge.api)).toBe(true); expect(bridge.api).toHaveProperty('attachTerminal'); expect(bridge.api).toHaveProperty('closeTab'); expect(bridge.api).not.toHaveProperty('ipcRenderer'); expect(bridge.api).not.toHaveProperty('send');
});
it('rejects invalid input locally and validates main responses', async () => {
  const id = { tabId: randomUUID(), generation: randomUUID() };
  expect(await bridge.api!.writeTerminal!({ ...id, data: '\0' })).toMatchObject({ ok: false, error: { code: 'VALIDATION' } }); expect(bridge.invoke).not.toHaveBeenCalled();
  bridge.invoke.mockResolvedValueOnce({ ok: true, value: { written: true, secret: 'bad' } }); expect(await bridge.api!.writeTerminal!({ ...id, data: '\x03' })).toMatchObject({ ok: false, error: { code: 'INTERNAL' } });
  bridge.invoke.mockResolvedValueOnce({ ok: true, value: { written: true } }); expect(await bridge.api!.writeTerminal!({ ...id, data: 'pwd\r' })).toEqual({ ok: true, value: { written: true } });
});
it('validates terminal event identities, shapes and byte limits but keeps VT control bytes', () => {
  const receive = vi.fn(), stop = bridge.api!.subscribeTerminal!(receive), id = { tabId: randomUUID(), generation: randomUUID() };
  const dispatch = (value: unknown) => { for (const listener of bridge.listeners.get('manager:terminal') ?? []) listener({}, value); };
  dispatch({ type: 'data', ...id, sequence: 1, data: '\x1b[31m雪\0' }); expect(receive).toHaveBeenCalledTimes(1);
  dispatch({ type: 'data', ...id, sequence: 2, data: 'x', unknown: true }); dispatch({ type: 'data', ...id, sequence: 3, data: '雪'.repeat(10000) }); dispatch({ type: 'data', ...id, generation: 'bad', sequence: 4, data: 'x' }); expect(receive).toHaveBeenCalledTimes(1);
  stop(); dispatch({ type: 'exit', ...id, exitCode: 0, signal: null, lastSequence: 1 }); expect(receive).toHaveBeenCalledTimes(1); expect(bridge.removed).toHaveBeenCalledWith('manager:terminal');
});
