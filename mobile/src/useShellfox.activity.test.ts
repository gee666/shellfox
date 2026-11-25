// @vitest-environment jsdom
import { act, cleanup, renderHook } from '@testing-library/react';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import type { NetworkPeer, NetworkStatus } from './network';
import type { Preferences } from './preferences';
import { snapshotSchema } from '../../src/shared/schemas';
import { sessionDot, tabDot } from '../../src/shared/terminal-status';
import { flush, ids, session } from './client/test-fixtures';
import { TestNative } from './rust-client/test-native';

// The real Rust client adapter over an inert native module.
const mocks = vi.hoisted(() => ({
  appState: { currentState: 'active', addEventListener: vi.fn() },
  native: undefined as TestNative | undefined,
}));
vi.mock('react-native', () => ({ AppState: mocks.appState }));
vi.mock('./network', () => ({
  nativeNetwork: { start: async () => '', stop: async () => undefined, login: async () => '', logout: async () => undefined },
  networkStatus: async (): Promise<NetworkStatus> => ({ status: 'online', loginUrl: null, error: null }),
  discoverHosts: async () => ({ peers: [host], discovering: false }),
}));
vi.mock('./rust-client/create-client', async () => {
  const { RustHostClient } = await import('./rust-client/rust-host-client');
  return { createNativeClient: (peerId: string, options: import('./client/host-client').RemoteClientOptions) =>
    new RustHostClient(peerId, mocks.native!, options) };
});
vi.mock('./preferences', () => ({
  defaultPreferences: { hostname: 'shellfox-mobile', remembered: [], fontSize: 14 },
  loadPreferences: async (): Promise<Preferences> => ({ hostname: 'shellfox-mobile', remembered: [host.id], fontSize: 14 }),
  savePreferences: async () => undefined,
}));
const { useShellfox } = await import('./useShellfox');

const host: NetworkPeer = { id: 'host-1', hostname: 'workstation', online: true, addresses: ['100.64.0.2'] };
const snapshot = snapshotSchema.parse({
  revision: 1,
  sessions: [{ ...session, status: 'running', tabs: [{
    id: ids.tab, sessionId: ids.session, title: 'agent', cwd: '/work', ordinal: 0, createdAt: session.createdAt,
    lifecycle: 'open', status: 'running', agents: 1, monitoringReason: null, error: null,
  }] }],
  settings: { version: 1, pythonPath: null, accentColor: '#abcdef', backgroundColor: '#111016', adapterId: 'embedded-pty', shellId: 'bash', shellExecutable: null, processRules: [], historyPageSize: 10, sessionSettleHours: 24, sessionDeleteHours: 720 },
  probe: { platform: 'linux', arch: 'x64', adapterId: 'embedded-pty', available: true, terminalVersion: null, shells: [], reasons: [], capabilities: { createWindow: false, addTab: true, focusWindow: false, activateTab: false, splitPane: false, attachExisting: false, closeTerminal: true, commandExitStatus: false, processTracking: false, explorerContextMenu: false } },
  explorer: { supported: false, installed: false, folderItemInstalled: false, backgroundInstalled: false, reason: null },
  cli: { supported: false, installed: false, command: 'shellfox', reason: null },
});
let changeState: (state: string) => void = () => undefined;
async function settle(ms = 200) { await act(async () => { await vi.advanceTimersByTimeAsync(ms); await flush(); }); }

beforeEach(() => {
  vi.useFakeTimers();
  mocks.appState.currentState = 'active';
  mocks.appState.addEventListener.mockImplementation((_event: string, listener: (state: string) => void) => {
    changeState = listener; return { remove: () => undefined };
  });
  const native = new TestNative();
  native.result = method => method === 'getSnapshot' ? { ok: true, value: snapshot } : { ok: true, value: {} };
  mocks.native = native;
});
afterEach(() => { cleanup(); vi.useRealTimers(); });

it('shows an agent that was busy before the app connected, on a cold start and after resume', async () => {
  const native = mocks.native!;
  const { result } = renderHook(useShellfox); await settle();
  const dots = () => {
    const machine = result.current.machines[0], loaded = machine.snapshot?.sessions[0];
    if (!loaded) throw new Error('Missing session');
    return [sessionDot(loaded, machine.busyTabIds).kind, tabDot(loaded.tabs[0], machine.busyTabIds[ids.tab]).kind];
  };
  // Hosts report tabs that are already busy in the first poll after pairing, then only changes.
  const seed = async () => {
    const poll = native.polls.at(-1);
    if (!poll) throw new Error('Missing poll');
    await act(async () => {
      native.event(poll, { ok: true, value: { events: [{ type: 'terminal', event: { type: 'activity', tabId: ids.tab, busy: true } }] } });
      await flush();
    });
    await settle();
  };
  expect(result.current.machines[0].state.status).toBe('connected');
  await seed(); expect(dots()).toEqual(['busy', 'busy']);

  await act(async () => { changeState('background'); await flush(); });
  expect(result.current.machines[0].busyTabIds).toEqual({});
  await act(async () => { changeState('active'); await flush(); }); await settle();
  expect(native.reconnect).toHaveBeenCalledOnce();
  await seed(); expect(dots()).toEqual(['busy', 'busy']);
});
