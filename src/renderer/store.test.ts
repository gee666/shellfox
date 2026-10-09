import { describe, expect, it, vi } from 'vitest';
import { failure, success } from '../shared/contracts';
import type { ChangedEvent, ManagerSnapshot, Result, TerminalEvent } from '../shared/contracts';
import { createManagerClient } from './store';
import { sessionDot, tabDot } from './components';
import { clampSidebarWidth } from './sidebar-width';
import { deferred, mockApi, session, snapshot } from './test-fixtures';

const flush = async () => { for (let i = 0; i < 8; i += 1) await Promise.resolve(); };

describe('manager client', () => {
  it('subscribes before the first snapshot and refetches an invalidation during fetch', async () => {
    const fixture = mockApi();
    const first = deferred<Result<ManagerSnapshot>>();
    fixture.api.getSnapshot.mockReturnValueOnce(first.promise);
    const client = createManagerClient(fixture.api);
    client.start();
    expect(fixture.api.subscribe.mock.invocationCallOrder[0]).toBeLessThan(fixture.api.getSnapshot.mock.invocationCallOrder[0]!);
    fixture.emit({ ...snapshot([session(2)]), revision: 2 });
    first.resolve(success(snapshot()));
    await flush();
    expect(fixture.api.getSnapshot).toHaveBeenCalledTimes(2);
    expect(client.store.getState().snapshot?.revision).toBe(2);
    client.stop();
    expect(fixture.unsubscribe).toHaveBeenCalledTimes(1);
  });

  it('does not replace newer state with an older revision', async () => {
    const fixture = mockApi({ ...snapshot(), revision: 5 });
    const client = createManagerClient(fixture.api);
    client.start(); await flush();
    fixture.api.getSnapshot.mockResolvedValueOnce(success({ ...snapshot(), revision: 4 }));
    client.refresh(); await flush();
    expect(client.store.getState().snapshot?.revision).toBe(5);
    client.stop();
  });

  it('ignores an in-flight response after unmount', async () => {
    const fixture = mockApi();
    const first = deferred<Result<ManagerSnapshot>>();
    fixture.api.getSnapshot.mockReturnValueOnce(first.promise);
    const client = createManagerClient(fixture.api);
    client.start(); client.stop(); first.resolve(success(snapshot())); await flush();
    expect(client.store.getState().snapshot).toBeNull();
  });

  it('handles a StrictMode stop/start while a request is pending', async () => {
    const fixture = mockApi();
    const first = deferred<Result<ManagerSnapshot>>();
    fixture.api.getSnapshot.mockReturnValueOnce(first.promise);
    const client = createManagerClient(fixture.api);
    client.start(); client.stop(); client.start();
    first.resolve(success({ ...snapshot(), revision: 99 })); await flush();
    expect(client.store.getState().snapshot?.revision).toBe(1);
    expect(fixture.unsubscribe).toHaveBeenCalledTimes(1);
    client.stop();
  });

  it('keeps an IPC failure visible and retries only a read when requested', async () => {
    const fixture = mockApi();
    fixture.api.getSnapshot.mockResolvedValueOnce(failure('STORAGE_FAILED', 'Database is unavailable.'));
    const client = createManagerClient(fixture.api);
    client.start(); await flush();
    expect(client.store.getState().error?.code).toBe('STORAGE_FAILED');
    expect(client.store.getState().loading).toBe(false);
    client.refresh(); await flush();
    expect(client.store.getState().snapshot).not.toBeNull();
    expect(fixture.api.createSession).not.toHaveBeenCalled();
    client.stop();
  });

  it('keeps shared selection separate from native controls', async () => {
    const fixture = mockApi();
    const client = createManagerClient(fixture.api);
    client.start(); await flush();
    client.select(session(2));
    expect(client.store.getState().selectedId).toBe(session(2).id);
    expect(fixture.api.createSession).not.toHaveBeenCalled();
    expect(fixture.api.addTab).not.toHaveBeenCalled();
    expect(fixture.api.focusSession).not.toHaveBeenCalled();
    client.stop();
  });

  it('does not activate or relaunch sessions on selection, including sessions with no live tabs', async () => {
    const item = session(); item.tabs[0]!.lifecycle = 'closed';
    const fixture = mockApi(snapshot([item])); const client = createManagerClient(fixture.api);
    client.start(); await flush(); client.select(item);
    expect(client.store.getState().activeTabIds[item.id]).toBeUndefined();
    expect(fixture.api.activateSession).not.toHaveBeenCalled();
    expect(fixture.api.addTab).not.toHaveBeenCalled();
    expect(fixture.api.createSession).not.toHaveBeenCalled();
    client.stop();
  });

  it('separates archive membership invalidation from native status polls', async () => {
    const initial = snapshot();
    const fixture = mockApi(initial); const client = createManagerClient(fixture.api);
    client.start(); await flush();
    const archiveVersion = client.store.getState().archiveVersion;
    fixture.emit({ ...initial, revision: 2 }, 'native'); await flush();
    expect(client.store.getState().archiveVersion).toBe(archiveVersion);
    expect(client.store.getState().historyStatusVersion).toBe(1);
    fixture.emit({ ...initial, revision: 3 }, 'history'); await flush();
    expect(client.store.getState().archiveVersion).toBe(archiveVersion + 1);
    fixture.emit({ ...snapshot([]), revision: 4 }, 'sessions'); await flush();
    expect(client.store.getState().archiveVersion).toBe(archiveVersion + 2);
    client.stop();
  });

  it('ignores closed tab selection and repairs stale active tab IDs on refresh', async () => {
    const item = session(); const fixture = mockApi(snapshot([item]));
    const client = createManagerClient(fixture.api); client.start(); await flush();
    const tabId = item.tabs[0]!.id;
    expect(client.store.getState().activeTabIds[item.id]).toBe(tabId);
    item.tabs[0]!.lifecycle = 'closed';
    fixture.emit({ ...snapshot([item]), revision: 2 }); await flush();
    client.selectTab(item, tabId);
    expect(client.store.getState().activeTabIds[item.id]).toBeUndefined();
    client.stop();
  });

  it('selects sessions requested by changed events without launching or activating', async () => {
    const fixture = mockApi(); const client = createManagerClient(fixture.api);
    client.start(); await flush();
    const listener = fixture.api.subscribe.mock.calls[0]![0];
    listener({ revision: 1, reason: 'sessions', selectSessionId: session(2).id } as ChangedEvent);
    await flush(); expect(client.store.getState().selectedId).toBe(session(2).id);
    expect(fixture.api.activateSession).not.toHaveBeenCalled(); client.stop();
  });

  it('drops the selection of a deleted archived session and re-reads the snapshot', async () => {
    const live = session(2), archived = session(1, { status: 'settled', settledAt: '2026-10-04T11:00:00.000Z' });
    const fixture = mockApi(snapshot([live])); const client = createManagerClient(fixture.api);
    client.start(); await flush();
    client.select(archived);
    expect(client.store.getState()).toMatchObject({ selectedId: archived.id, historical: archived });
    const { archiveVersion, historyStatusVersion, selectionEpoch } = client.store.getState();
    client.remove(archived.id); await flush();
    const state = client.store.getState();
    expect(state).toMatchObject({ selectedId: live.id, historical: null });
    expect(state.archiveVersion).toBe(archiveVersion + 1); expect(state.historyStatusVersion).toBe(historyStatusVersion + 1); expect(state.selectionEpoch).toBe(selectionEpoch + 1);
    expect(fixture.api.getSnapshot).toHaveBeenCalledTimes(2);
    client.select(live); client.remove(archived.id); await flush();
    expect(client.store.getState().selectedId).toBe(live.id); client.stop();
  });

  it('tracks activity independently from snapshots and clears busy on exit', async () => {
    const fixture = mockApi(); const client = createManagerClient(fixture.api); client.start(); await flush();
    const tab = session().tabs[0]!;
    fixture.emitTerminal({ type: 'activity', tabId: tab.id, busy: true } as TerminalEvent);
    expect(client.store.getState().busyTabIds[tab.id]).toBe(true);
    fixture.emitTerminal({ type: 'exit', tabId: tab.id, generation: tab.generation, exitCode: 0, signal: null, lastSequence: 0 });
    expect(client.store.getState().busyTabIds[tab.id]).toBe(false);
    expect(fixture.api.getSnapshot).toHaveBeenCalledTimes(1); client.stop();
    fixture.emitTerminal({ type: 'activity', tabId: tab.id, busy: true } as TerminalEvent);
    expect(client.store.getState().busyTabIds[tab.id]).toBe(false);
  });

  it('does not notify subscribers for repeated terminal activity', async () => {
    const fixture = mockApi(); const client = createManagerClient(fixture.api);
    client.start(); await flush();
    const listener = vi.fn(); const unsubscribe = client.store.subscribe(listener);
    const tabId = session().tabs[0]!.id;
    for (let i = 0; i < 100; i += 1) fixture.emitTerminal({ type: 'activity', tabId, busy: true } as TerminalEvent);
    expect(listener).toHaveBeenCalledTimes(1);
    for (let i = 0; i < 100; i += 1) fixture.emitTerminal({ type: 'exit', tabId } as TerminalEvent);
    expect(listener).toHaveBeenCalledTimes(2);
    unsubscribe(); client.stop();
  });

  it('compares large snapshots without per-session array searches', async () => {
    const sessions = Array.from({ length: 2000 }, (_, index) => session(index + 1));
    const fixture = mockApi(snapshot(sessions)); const client = createManagerClient(fixture.api);
    client.start(); await flush();
    const version = client.store.getState().archiveVersion;
    const find = vi.spyOn(sessions, 'find');
    fixture.emit({ ...snapshot(sessions), revision: 2 }, 'native'); await flush();
    expect(client.store.getState().archiveVersion).toBe(version);
    expect(find.mock.calls.length).toBeLessThan(5);
    find.mockRestore(); client.stop();
  });

  it('maps dots with error > busy agent > idle agent > shell and ignores closed tabs', () => {
    const item = session(); const tab = item.tabs[0]!;
    expect(tabDot(tab, true).kind).toBe('shell');
    tab.status = 'running'; expect(tabDot(tab).title).toBe('Agent idle');
    expect(tabDot(tab, true).title).toBe('Agent working');
    const error = { ...session(2).tabs[0]!, status: 'error' as const, error: { code: 'LAUNCH_FAILED' as const, message: 'Failed', retryable: true } };
    item.tabs.push(error);
    expect(sessionDot(item, { [tab.id]: true }).title).toBe('Error: Failed');
    error.lifecycle = 'closed';
    expect(sessionDot(item, { [tab.id]: true }).kind).toBe('busy');
    expect(sessionDot(item, {}).kind).toBe('running');
    tab.lifecycle = 'closed'; expect(sessionDot(item, {}).kind).toBe('shell');
  });

  it('clamps sidebar width to 160px through half the window', () => {
    expect(clampSidebarWidth(0, 1000)).toBe(160);
    expect(clampSidebarWidth(900, 1000)).toBe(500);
    expect(clampSidebarWidth(NaN, 1000)).toBe(220);
  });

  it('does not leak updates from the subscription after cleanup', async () => {
    const fixture = mockApi();
    const client = createManagerClient(fixture.api);
    client.start(); await flush(); client.stop();
    const listener = vi.fn();
    const unsubscribe = client.store.subscribe(listener);
    fixture.emit({ ...snapshot(), revision: 2 });
    expect(listener).not.toHaveBeenCalled();
    unsubscribe();
  });
});
