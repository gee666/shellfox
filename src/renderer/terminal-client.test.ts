// @vitest-environment jsdom
import './terminal-test-mocks';
import { terminalMocks } from './terminal-test-mocks';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { success, failure } from '../shared/contracts';
import type { Result, TerminalAttachmentDto, TerminalDataEvent } from '../shared/contracts';
import { TerminalRegistry, clipboardShortcut, createTerminalSurface, type SurfaceFactory } from './terminal-client';
import { deferred, mockApi, session } from './test-fixtures';

const tab = session().tabs[0]!;
const flush = async () => { for (let i = 0; i < 30; i++) await Promise.resolve(); };
const chunk = (sequence: number, data = `${sequence}`, generation = tab.generation): TerminalDataEvent => ({ type: 'data', tabId: tab.id, generation, sequence, data });
const attachment = (chunks: TerminalDataEvent[] = [], extras: Partial<TerminalAttachmentDto> = {}): TerminalAttachmentDto => ({
  tabId: tab.id, sessionId: tab.sessionId, generation: tab.generation,
  firstSequence: chunks[0]?.sequence ?? 1, lastSequence: chunks.at(-1)?.sequence ?? 0,
  chunks, truncated: false, state: 'open', exitCode: null, cols: 80, rows: 24, lifetime: 'app-owned', ...extras,
});
function surfaces(delayed = false) {
  const values: { output: string; input: (data: string, userInitiated?: boolean) => void; callbacks: (() => void)[]; dispose: ReturnType<typeof vi.fn>; fit: ReturnType<typeof vi.fn>; focus: ReturnType<typeof vi.fn> }[] = [];
  const factory: SurfaceFactory = input => {
    const item = { output: '', input, callbacks: [] as (() => void)[], dispose: vi.fn(), fit: vi.fn(() => ({ cols: 80, rows: 24 })), focus: vi.fn() };
    values.push(item);
    return { element: document.createElement('div'), write(data, done) { item.output += data; if (delayed) item.callbacks.push(done); else done(); }, reset() { item.output = ''; }, fit: item.fit, focus: item.focus, setInput() {}, dispose: item.dispose };
  };
  return { values, factory, latest: () => values.at(-1)! };
}
const registries: TerminalRegistry[] = [];
afterEach(() => { registries.splice(0).forEach(registry => registry.dispose()); document.body.replaceChildren(); });
function setup(delayed = false) {
  const fixture = mockApi(); const screens = surfaces(delayed); const registry = new TerminalRegistry(fixture.api, screens.factory);
  registries.push(registry); registry.start();
  const host = document.createElement('div'); document.body.append(host);
  const unmount = registry.mount(tab.id, host, true);
  return { ...fixture, ...screens, registry, host, unmount };
}

describe('terminal stream ownership', () => {
  it('does not steal tab rename focus when an attachment finishes late', async () => {
    const fixture = setup(); await flush(); fixture.latest().focus.mockClear();
    const pending = deferred<Result<TerminalAttachmentDto>>();
    fixture.api.attachTerminal.mockReturnValueOnce(pending.promise);
    fixture.registry.refresh(tab.id);
    const input = document.createElement('input'); input.className = 'tab-rename'; document.body.append(input); input.focus();
    pending.resolve(success(attachment())); await flush();
    fixture.registry.focus(tab.id);
    expect(fixture.latest().focus).not.toHaveBeenCalled(); expect(document.activeElement).toBe(input);
    input.remove(); fixture.registry.focus(tab.id); expect(fixture.latest().focus).toHaveBeenCalledOnce();
  });
  it('subscribes before attach and deduplicates live output arriving during replay', async () => {
    const fixture = setup(); const pending = deferred<Result<TerminalAttachmentDto>>();
    // Initial mount already attached; defer the explicit next read.
    await flush(); fixture.api.attachTerminal.mockReturnValueOnce(pending.promise);
    fixture.registry.refresh(tab.id);
    fixture.emitTerminal(chunk(1, 'a')); fixture.emitTerminal(chunk(2, 'b'));
    pending.resolve(success(attachment([chunk(1, 'a'), chunk(2, 'b')]))); await flush();
    expect(fixture.api.subscribeTerminal.mock.invocationCallOrder[0]).toBeLessThan(fixture.api.attachTerminal.mock.invocationCallOrder[0]!);
    expect(fixture.latest().output).toBe('ab');
    fixture.emitTerminal(chunk(2, 'duplicate')); expect(fixture.latest().output).toBe('ab');
  });
  it('recovers a sequence gap from replay and renders reordered data once', async () => {
    const fixture = setup(); await flush();
    fixture.api.attachTerminal.mockResolvedValueOnce(success(attachment([chunk(1, 'a'), chunk(2, 'b'), chunk(3, 'c')])));
    fixture.emitTerminal(chunk(3, 'c')); fixture.emitTerminal(chunk(2, 'b')); await flush();
    expect(fixture.latest().output).toBe('abc');
    expect(fixture.api.detachTerminal).toHaveBeenCalledWith({ tabId: tab.id, generation: tab.generation });
    expect(fixture.api.attachTerminal).toHaveBeenLastCalledWith({ tabId: tab.id });
  });
  it('reports an inconsistent replay gap instead of busy-looping or restarting a shell', async () => {
    const fixture = setup(); await flush();
    fixture.api.attachTerminal.mockResolvedValueOnce(success(attachment([chunk(3, 'missing-prefix')], { firstSequence: 1, lastSequence: 3 })));
    fixture.emitTerminal(chunk(3, 'missing-prefix')); await flush();
    expect(fixture.registry.getState(tab.id)).toMatchObject({ phase: 'unavailable', error: { code: 'INTERNAL' } });
    expect(fixture.api.attachTerminal).toHaveBeenCalledTimes(2); expect(fixture.api.activateSession).not.toHaveBeenCalled();
    fixture.api.attachTerminal.mockResolvedValueOnce(success(attachment([chunk(3, 'retained')], { firstSequence: 3, lastSequence: 3, truncated: true })));
    fixture.registry.refresh(tab.id); await flush(); expect(fixture.latest().output).toBe('retained');
  });
  it('takes generation from authoritative attachment and fences old output/exit and parser callbacks', async () => {
    const fixture = setup(true); await flush();
    fixture.emitTerminal(chunk(1, 'old')); const old = fixture.latest();
    const generation = '00000000-0000-4000-b000-000000000001';
    fixture.api.attachTerminal.mockResolvedValueOnce(success(attachment([chunk(1, 'new', generation)], { generation, truncated: true })));
    fixture.emitTerminal(chunk(1, 'new', generation)); await flush();
    old.callbacks.forEach(done => done()); old.input('stale terminal reply'); await flush();
    expect(fixture.api.writeTerminal).not.toHaveBeenCalled();
    fixture.emitTerminal(chunk(2, 'stale'));
    fixture.emitTerminal({ type: 'exit', tabId: tab.id, generation: tab.generation, lastSequence: 2, exitCode: 9, signal: null });
    expect(fixture.latest().output).toBe('new'); expect(old.dispose).toHaveBeenCalledTimes(1);
    expect(fixture.registry.getState(tab.id)).toMatchObject({ generation, phase: 'open' });
  });
  it('resyncs when a new generation arrives after an attachment snapshot was taken', async () => {
    const fixture = setup(); await flush(); const pending = deferred<Result<TerminalAttachmentDto>>();
    const generation = '00000000-0000-4000-b000-000000000002';
    fixture.api.attachTerminal.mockReturnValueOnce(pending.promise).mockResolvedValueOnce(success(attachment([chunk(1, 'new', generation)], { generation, truncated: true })));
    fixture.registry.refresh(tab.id); fixture.emitTerminal(chunk(1, 'new', generation));
    pending.resolve(success(attachment())); await flush();
    expect(fixture.registry.getState(tab.id).generation).toBe(generation); expect(fixture.latest().output).toBe('new');
  });
  it('resets a truncated replay and states that older output is unavailable', async () => {
    const fixture = setup(); await flush(); fixture.emitTerminal(chunk(1, 'old'));
    fixture.api.attachTerminal.mockResolvedValueOnce(success(attachment([chunk(8, 'retained')], { truncated: true, firstSequence: 8, lastSequence: 8 })));
    fixture.registry.refresh(tab.id); await flush();
    expect(fixture.latest().output).toBe('retained'); expect(fixture.registry.getState(tab.id).warning).toMatch(/Earlier output is unavailable/);
  });
  it('replays a snapshot attachment at the grid it was taken at, without an unavailable-output warning', async () => {
    const order: string[] = [];
    const factory: SurfaceFactory = () => ({ element: document.createElement('div'), write(data, done) { order.push(`write:${data}`); done(); }, reset() {}, fit: () => ({ cols: 120, rows: 30 }), resize: (cols, rows) => { order.push(`resize:${cols}x${rows}`); }, focus() {}, setInput() {}, dispose() {} });
    const fixture = mockApi(); const registry = new TerminalRegistry(fixture.api, factory); registries.push(registry); registry.start();
    fixture.api.attachTerminal.mockResolvedValueOnce(success(attachment([chunk(8, 'screen'), chunk(9, 'live')], { truncated: true, snapshot: true, firstSequence: 8, lastSequence: 9, cols: 132, rows: 43 })));
    const host = document.createElement('div'); document.body.append(host); registry.mount(tab.id, host, true); await flush();
    expect(order.slice(0, 2)).toEqual(['resize:132x43', 'write:screen']);
    expect(registry.getState(tab.id)).toMatchObject({ phase: 'open', warning: null });
    // The grid is changed to the viewport only afterwards, through the normal fit/resize path.
    expect(fixture.api.resizeTerminal).toHaveBeenCalledWith({ tabId: tab.id, generation: tab.generation, cols: 120, rows: 30 });
  });
  it('does not fit a replay to the viewport until all asynchronous writes have parsed', async () => {
    const f = setup(true); await flush(); f.emitTerminal(chunk(1, 'pending old output'));
    f.api.attachTerminal.mockResolvedValueOnce(success(attachment([chunk(8, 'screen'), chunk(9, 'diff')], { snapshot: true, truncated: true, cols: 132, rows: 43 })));
    f.registry.refresh(tab.id); await flush();
    const screen = f.latest();
    f.registry.fit(tab.id); f.registry.fit(tab.id);
    expect(screen.fit).not.toHaveBeenCalled();
    screen.callbacks.shift()!(); await flush();
    expect(screen.fit).not.toHaveBeenCalled();
    screen.callbacks.shift()!(); await flush();
    expect(screen.fit).toHaveBeenCalledOnce(); expect(screen.output).toBe('screendiff');
    expect(f.values).toHaveLength(2); // Initial surface, then one recovery surface.
  });
  it('also sizes the grid before a raw full replay, so cursor-addressed output lands where the app drew it', async () => {
    const order: string[] = [];
    const factory: SurfaceFactory = () => ({ element: document.createElement('div'), write(data, done) { order.push(`write:${data}`); done(); }, reset() {}, fit: () => ({ cols: 100, rows: 40 }), resize: (cols, rows) => { order.push(`resize:${cols}x${rows}`); }, focus() {}, setInput() {}, dispose() {} });
    const fixture = mockApi(); const registry = new TerminalRegistry(fixture.api, factory); registries.push(registry); registry.start();
    fixture.api.attachTerminal.mockResolvedValueOnce(success(attachment([chunk(8, 'tail')], { truncated: true, firstSequence: 8, lastSequence: 8, cols: 100, rows: 40 })));
    const host = document.createElement('div'); document.body.append(host); registry.mount(tab.id, host, true); await flush();
    expect(order.slice(0, 2)).toEqual(['resize:100x40', 'write:tail']);
    expect(registry.getState(tab.id).warning).toMatch(/Earlier output is unavailable/);
  });
  it('makes the app repaint after a view was rebuilt from a partial raw tail, and not otherwise', async () => {
    vi.useFakeTimers();
    try {
      const fixture = setup(); await vi.advanceTimersByTimeAsync(10); await flush(); fixture.api.resizeTerminal.mockClear(); fixture.emitTerminal(chunk(1, 'old')); await flush();
      fixture.api.attachTerminal.mockResolvedValueOnce(success(attachment([chunk(8, 'tail')], { truncated: true, firstSequence: 8, lastSequence: 8 })));
      fixture.registry.refresh(tab.id); await vi.advanceTimersByTimeAsync(300); await flush();
      // SIGWINCH is only observable when the size really changes: shrink, wait, restore.
      expect(fixture.api.resizeTerminal.mock.calls.map(([size]) => `${size.cols}x${size.rows}`)).toEqual(['80x23', '80x24']);
      fixture.api.resizeTerminal.mockClear(); fixture.registry.refresh(tab.id); await vi.advanceTimersByTimeAsync(300); await flush();
      expect(fixture.api.resizeTerminal).not.toHaveBeenCalled();
    } finally { vi.useRealTimers(); }
  });
  it('drains output while hidden without resizing, focusing or allowing hidden input', async () => {
    const fixture = setup(); await flush(); fixture.api.resizeTerminal.mockClear(); fixture.latest().focus.mockClear();
    fixture.registry.visibility(tab.id, false); fixture.emitTerminal(chunk(1, 'background')); fixture.latest().input('hidden'); fixture.registry.fit(tab.id); await flush();
    expect(fixture.latest().output).toBe('background'); expect(fixture.api.resizeTerminal).not.toHaveBeenCalled(); expect(fixture.api.writeTerminal).not.toHaveBeenCalled();
    fixture.registry.visibility(tab.id, true); fixture.latest().input('visible'); await flush();
    expect(fixture.api.writeTerminal).toHaveBeenCalledWith({ tabId: tab.id, generation: tab.generation, data: 'visible' });
  });
  it('allows parser device replies during hidden output without enabling arbitrary hidden input', async () => {
    const fixture = setup(true); await flush(); fixture.registry.visibility(tab.id, false);
    fixture.latest().input('hidden keystroke'); await flush(); expect(fixture.api.writeTerminal).not.toHaveBeenCalled();
    fixture.emitTerminal(chunk(1, '\u001b[6n')); fixture.latest().input('\u001b[1;1R'); await flush();
    expect(fixture.api.writeTerminal).toHaveBeenCalledWith({ tabId: tab.id, generation: tab.generation, data: '\u001b[1;1R' });
  });
  it('preserves its screen through host unmount/reattach and never spawns or closes a shell', async () => {
    const fixture = setup(); await flush(); fixture.emitTerminal(chunk(1, 'screen'));
    const screen = fixture.latest(); fixture.unmount(); fixture.emitTerminal(chunk(2, ' hidden'));
    fixture.registry.mount(tab.id, fixture.host, true); await flush();
    expect(fixture.latest()).toBe(screen); expect(screen.output).toBe('screen hidden');
    expect(fixture.api.createSession).not.toHaveBeenCalled(); expect(fixture.api.activateSession).not.toHaveBeenCalled(); expect(fixture.api.closeTab).not.toHaveBeenCalled();
  });
  it('acknowledges only contiguous parsed output, not queued or duplicate chunks', async () => {
    const fixture = setup(true); await flush(); fixture.emitTerminal(chunk(1, 'one')); fixture.emitTerminal(chunk(2, 'two'));
    expect(fixture.api.acknowledgeTerminal).not.toHaveBeenCalled();
    fixture.latest().callbacks.shift()!(); await flush();
    expect(fixture.api.acknowledgeTerminal).toHaveBeenLastCalledWith({ tabId: tab.id, generation: tab.generation, sequence: 1 });
    fixture.latest().callbacks.shift()!(); await flush(); fixture.emitTerminal(chunk(2, 'duplicate'));
    expect(fixture.api.acknowledgeTerminal).toHaveBeenLastCalledWith({ tabId: tab.id, generation: tab.generation, sequence: 2 });
    expect(fixture.api.acknowledgeTerminal).toHaveBeenCalledTimes(2);
  });
  it('detaches without closing and waits for late detach before reattaching the same generation', async () => {
    const fixture = setup(); await flush(); const pending = deferred<Result<{ detached: true }>>();
    fixture.api.detachTerminal.mockReturnValueOnce(pending.promise); fixture.unmount(); await flush();
    expect(fixture.api.detachTerminal).toHaveBeenCalledWith({ tabId: tab.id, generation: tab.generation });
    fixture.registry.mount(tab.id, fixture.host, true); await flush(); expect(fixture.api.attachTerminal).toHaveBeenCalledTimes(1);
    pending.resolve(success({ detached: true })); await flush(); expect(fixture.api.attachTerminal).toHaveBeenCalledTimes(2);
    expect(fixture.api.closeTab).not.toHaveBeenCalled();
  });
  it('does not acknowledge old parser completions after a runtime generation replacement', async () => {
    const fixture = setup(true); await flush(); fixture.emitTerminal(chunk(1, 'old')); const old = fixture.latest();
    const generation = '00000000-0000-4000-b000-000000000009';
    fixture.api.attachTerminal.mockResolvedValueOnce(success(attachment([chunk(1, 'new', generation)], { generation, truncated: true })));
    fixture.emitTerminal(chunk(1, 'new', generation)); await flush(); old.callbacks.shift()!(); await flush();
    expect(fixture.api.acknowledgeTerminal).not.toHaveBeenCalled(); fixture.latest().callbacks.shift()!(); await flush();
    expect(fixture.api.acknowledgeTerminal).toHaveBeenCalledWith({ tabId: tab.id, generation, sequence: 1 });
  });
  it('validates input byte bounds/NUL and serializes accepted input without automatic retry', async () => {
    const fixture = setup(); await flush();
    fixture.latest().input('\0'); fixture.latest().input('界'.repeat(22000)); await flush(); expect(fixture.api.writeTerminal).not.toHaveBeenCalled();
    const pending = deferred<Result<{ written: true }>>(); fixture.api.writeTerminal.mockReturnValueOnce(pending.promise);
    fixture.latest().input('one'); fixture.latest().input('two'); await flush(); expect(fixture.api.writeTerminal).toHaveBeenCalledTimes(1);
    pending.resolve(failure('INTERNAL', 'Input not confirmed')); await flush();
    expect(fixture.api.writeTerminal.mock.calls.map(call => call[0].data)).toEqual(['one', 'two']);
    expect(fixture.registry.getState(tab.id).operationError?.message).toBe('Input not confirmed');
  });
  it('recovers ACK failures without publishing transient errors', async () => {
    const fixture = setup(); await flush();
    fixture.api.acknowledgeTerminal.mockResolvedValueOnce(failure('INTERNAL', 'Credit view expired'));
    const errors: unknown[] = [];
    fixture.registry.subscribe(tab.id, () => errors.push(fixture.registry.getState(tab.id).error));
    fixture.emitTerminal(chunk(1)); await flush();
    expect(errors.every(error => error === null)).toBe(true);
    expect(fixture.api.attachTerminal).toHaveBeenCalledTimes(2);
    expect(fixture.registry.getState(tab.id).phase).toBe('open');
  });
  it('ignores resize failures and retries the unconfirmed size on the next fit', async () => {
    const fixture = setup(); await flush();
    fixture.api.resizeTerminal.mockResolvedValueOnce(failure('TARGET_LOST', 'Runtime exited'));
    fixture.latest().fit.mockReturnValue({ cols: 100, rows: 30 });
    fixture.registry.fit(tab.id); await flush();
    expect(fixture.registry.getState(tab.id)).toMatchObject({ phase: 'open', error: null, operationError: null });
    fixture.registry.fit(tab.id); await flush();
    expect(fixture.api.resizeTerminal.mock.calls.filter(([size]) => size.cols === 100)).toHaveLength(2);
  });
  it('does not poison a retained screen with cleanup errors and can attach past a retired runtime', async () => {
    const fixture = setup(); await flush();
    fixture.api.detachTerminal.mockResolvedValue(failure('TARGET_LOST', 'Old runtime gone'));
    fixture.unmount(); await flush();
    expect(fixture.registry.getState(tab.id)).toMatchObject({ phase: 'open', error: null });
    fixture.registry.mount(tab.id, fixture.host, true); await flush();
    expect(fixture.api.attachTerminal).toHaveBeenCalledTimes(2);
    expect(fixture.registry.getState(tab.id)).toMatchObject({ phase: 'open', error: null });
  });
  it('keeps an unsafe detach failure actionable when it blocks attach and allows retry', async () => {
    const fixture = setup(); await flush();
    fixture.api.detachTerminal.mockResolvedValueOnce(failure('INTERNAL', 'Cannot detach view'));
    fixture.registry.refresh(tab.id); await flush();
    expect(fixture.registry.getState(tab.id)).toMatchObject({ phase: 'unavailable', error: { message: 'Cannot detach view' } });
    expect(fixture.api.attachTerminal).toHaveBeenCalledTimes(1);
    fixture.registry.refresh(tab.id); await flush();
    expect(fixture.registry.getState(tab.id)).toMatchObject({ phase: 'open', error: null });
  });
  it('does not publish failures from parser replies or writes whose view has unmounted', async () => {
    const fixture = setup(true); await flush();
    fixture.api.writeTerminal.mockResolvedValueOnce(failure('INTERNAL', 'Device reply rejected'));
    fixture.emitTerminal(chunk(1, '\u001b[6n')); fixture.latest().input('\u001b[1;1R'); await flush();
    expect(fixture.registry.getState(tab.id).operationError).toBeNull();
    fixture.latest().callbacks.shift()!(); await flush();
    const pending = deferred<Result<{ written: true }>>();
    fixture.api.writeTerminal.mockReturnValueOnce(pending.promise);
    fixture.latest().input('user command'); await flush(); fixture.unmount();
    pending.resolve(failure('INTERNAL', 'Late failure')); await flush();
    expect(fixture.registry.getState(tab.id).operationError).toBeNull();
    expect(fixture.api.writeTerminal).toHaveBeenCalledTimes(2);
  });
  it('keeps a user write failure actionable across same-generation display recovery', async () => {
    const fixture = setup(); await flush();
    const pending = deferred<Result<{ written: true }>>();
    fixture.api.writeTerminal.mockReturnValueOnce(pending.promise);
    fixture.latest().input('user command'); await flush();
    fixture.registry.refresh(tab.id); await flush();
    pending.resolve(failure('INTERNAL', 'Command input not confirmed')); await flush();
    expect(fixture.registry.getState(tab.id).operationError?.message).toBe('Command input not confirmed');
    expect(fixture.api.writeTerminal).toHaveBeenCalledTimes(1);
  });
  it('reports explicit user input failures even while output parsing is pending', async () => {
    const fixture = setup(true); await flush();
    fixture.emitTerminal(chunk(1, 'slow output'));
    fixture.api.writeTerminal.mockResolvedValueOnce(failure('INTERNAL', 'User input rejected'));
    fixture.latest().input('typed command', true); await flush();
    expect(fixture.registry.getState(tab.id).operationError?.message).toBe('User input rejected');
    expect(fixture.api.writeTerminal).toHaveBeenCalledTimes(1);
  });
  it('rejects hidden user input even while parsing, but accepts device replies', async () => {
    const f = setup(true); await flush(); f.registry.visibility(tab.id, false);
    f.emitTerminal(chunk(1, '\u001b[6n'));
    f.latest().input('hidden paste', true); f.latest().input('\u001b[1;1R', false); await flush();
    expect(f.api.writeTerminal.mock.calls.map(([input]) => input.data)).toEqual(['\u001b[1;1R']);
  });
  it('measures each admitted UTF-8 output chunk only once', async () => {
    const f = setup(); await flush();
    const encode = vi.spyOn(TextEncoder.prototype, 'encode');
    try {
      for (let i = 1; i <= 256; i++) f.emitTerminal(chunk(i, '界🙂'));
      await flush();
      expect(encode).toHaveBeenCalledTimes(256);
      expect(f.latest().output).toBe('界🙂'.repeat(256));
    } finally { encode.mockRestore(); }
  });
  it('cleans up a late initial attach before a replacement view can attach', async () => {
    const f = setup(); const pending = deferred<Result<TerminalAttachmentDto>>();
    f.api.attachTerminal.mockReturnValueOnce(pending.promise); await flush();
    const old = f.latest(); f.registry.dispose();
    expect(f.api.detachTerminal).not.toHaveBeenCalled();
    f.registry.start(); f.registry.mount(tab.id, f.host, true); await flush();
    expect(f.api.attachTerminal).toHaveBeenCalledTimes(1);
    const cleanup = deferred<Result<{ detached: true }>>(); f.api.detachTerminal.mockReturnValueOnce(cleanup.promise);
    pending.resolve(success(attachment([chunk(1, 'late')]))); await flush();
    expect(old.dispose).toHaveBeenCalledOnce(); expect(old.output).toBe('');
    expect(f.api.detachTerminal).toHaveBeenCalledExactlyOnceWith({ tabId: tab.id, generation: tab.generation });
    expect(f.api.attachTerminal).toHaveBeenCalledTimes(1);
    cleanup.resolve(success({ detached: true })); await flush();
    expect(f.api.attachTerminal).toHaveBeenCalledTimes(2);
    f.emitTerminal(chunk(1, 'replacement')); expect(f.latest().output).toBe('replacement');
    expect(f.api.closeTab).not.toHaveBeenCalled(); expect(f.api.activateSession).not.toHaveBeenCalled();
  });
  it('can retry a failed retired-view cleanup instead of leaking credit permanently', async () => {
    const f = setup(); const pending = deferred<Result<TerminalAttachmentDto>>();
    f.api.attachTerminal.mockReturnValueOnce(pending.promise); await flush(); f.registry.dispose();
    f.api.detachTerminal.mockResolvedValueOnce(failure('INTERNAL', 'Detach unavailable')).mockResolvedValueOnce(failure('INTERNAL', 'Detach unavailable'));
    f.registry.start(); f.registry.mount(tab.id, f.host, true);
    pending.resolve(success(attachment([chunk(1, 'late')]))); await flush();
    expect(f.registry.getState(tab.id)).toMatchObject({ phase: 'unavailable', error: { message: 'Detach unavailable' } });
    expect(f.api.attachTerminal).toHaveBeenCalledTimes(1);
    f.registry.refresh(tab.id); await flush();
    expect(f.registry.getState(tab.id)).toMatchObject({ phase: 'open', error: null });
    expect(f.api.attachTerminal).toHaveBeenCalledTimes(2); expect(f.api.closeTab).not.toHaveBeenCalled();
  });
  it('serializes late attach cleanup across LRU eviction and reopening the same tab', async () => {
    const f = setup(); const pending = deferred<Result<TerminalAttachmentDto>>();
    f.api.attachTerminal.mockReturnValueOnce(pending.promise); await flush();
    const old = f.latest(); f.unmount();
    for (let i = 2; i <= 40; i++) {
      const unmount = f.registry.mount(session(i).tabs[0].id, f.host, true); await flush(); unmount();
    }
    expect(old.dispose).toHaveBeenCalledOnce();
    expect(f.values.filter(value => !value.dispose.mock.calls.length)).toHaveLength(24);
    f.registry.mount(tab.id, f.host, true); await flush();
    expect(f.api.attachTerminal.mock.calls.filter(([input]) => input.tabId === tab.id)).toHaveLength(1);
    pending.resolve(success(attachment([chunk(1, 'stale')]))); await flush();
    expect(f.api.attachTerminal.mock.calls.filter(([input]) => input.tabId === tab.id)).toHaveLength(2);
    expect(old.output).toBe(''); expect(f.latest().output).toBe('');
    expect(f.api.closeTab).not.toHaveBeenCalled();
  });
  it('trims on the last unsubscribe without disposing a mounted or subscribed screen', async () => {
    const f = setup(); await flush(); f.unmount();
    const unsubscribers: (() => void)[] = [];
    for (let i = 2; i <= 30; i++) {
      const id = session(i).tabs[0].id;
      unsubscribers.push(f.registry.subscribe(id, () => {}));
      const unmount = f.registry.mount(id, f.host, true); await flush(); unmount();
    }
    expect(f.values.filter(value => !value.dispose.mock.calls.length)).toHaveLength(29);
    unsubscribers.forEach(unsubscribe => unsubscribe());
    expect(f.values.filter(value => !value.dispose.mock.calls.length)).toHaveLength(24);
  });
  it('validates resize dimensions and coalesces unchanged sizes', async () => {
    const fixture = setup(); await flush(); fixture.api.resizeTerminal.mockClear();
    fixture.registry.fit(tab.id); fixture.registry.fit(tab.id); await flush(); expect(fixture.api.resizeTerminal).not.toHaveBeenCalled();
    fixture.latest().fit.mockReturnValue({ cols: 0, rows: 700 }); fixture.registry.fit(tab.id); await flush(); expect(fixture.api.resizeTerminal).not.toHaveBeenCalled();
    fixture.latest().fit.mockReturnValue({ cols: 100, rows: 30 }); fixture.registry.fit(tab.id); await flush();
    expect(fixture.api.resizeTerminal).toHaveBeenCalledWith({ tabId: tab.id, generation: tab.generation, cols: 100, rows: 30 });
  });
  it('bounds delayed display writes by resetting from backend replay', async () => {
    const fixture = setup(true); await flush(); fixture.api.attachTerminal.mockResolvedValue(success(attachment([chunk(33, 'retained')], { truncated: true, firstSequence: 33, lastSequence: 33 })));
    for (let i = 1; i <= 33; i++) fixture.emitTerminal(chunk(i, 'x'.repeat(16384)));
    await flush(); expect(fixture.latest().output).toBe('retained'); expect(fixture.registry.getState(tab.id).warning).toMatch(/retained terminal buffer/);
  });
  it('retains only 24 detached terminal screens and relies on attach replay after eviction', async () => {
    const fixture = setup(); await flush(); fixture.unmount();
    for (let i = 2; i < 28; i++) {
      const id = session(i).tabs[0]!.id; const unmount = fixture.registry.mount(id, fixture.host, true); await flush(); unmount();
    }
    expect(fixture.values.filter(value => !value.dispose.mock.calls.length).length).toBeLessThanOrEqual(24);
    expect(fixture.api.closeTab).not.toHaveBeenCalled();
  });
  it('ignores pending responses and live events after cleanup', async () => {
    const fixture = setup(); await flush(); const pending = deferred<Result<TerminalAttachmentDto>>();
    fixture.api.attachTerminal.mockReturnValueOnce(pending.promise); fixture.registry.refresh(tab.id); const screen = fixture.latest();
    fixture.registry.dispose(); pending.resolve(success(attachment([chunk(1, 'late')]))); fixture.emitTerminal(chunk(1, 'late')); await flush();
    expect(screen.output).toBe(''); expect(fixture.terminalUnsubscribe).toHaveBeenCalledTimes(1);
  });
  it('recovers stream errors quietly and preserves exit state without claiming command success', async () => {
    const fixture = setup(); await flush(); fixture.emitTerminal({ type: 'error', tabId: tab.id, generation: tab.generation, error: { code: 'MONITOR_UNAVAILABLE', message: 'Unavailable evidence', retryable: true } });
    expect(fixture.registry.getState(tab.id).error).toBeNull();
    fixture.emitTerminal({ type: 'exit', tabId: tab.id, generation: tab.generation, lastSequence: 0, exitCode: 3, signal: null }); await flush();
    expect(fixture.registry.getState(tab.id)).toMatchObject({ phase: 'closed', exitCode: 3 });
  });
});

describe('xterm safety configuration', () => {
  it('skips unchanged-grid repaints and pauses cursor blinking on detached screens', () => {
    const surface = createTerminalSurface(() => {}); document.body.append(surface.element);
    const terminal = terminalMocks.terminals.at(-1);
    const resize = vi.spyOn(terminal, 'resize');
    expect(terminal.options.cursorBlink).toBe(false);
    surface.setVisible?.(true); surface.fit();
    for (let i = 0; i < 20; i++) surface.fit();
    expect(terminal.refresh).toHaveBeenCalledOnce(); expect(resize).not.toHaveBeenCalled();
    const dimensions = vi.spyOn(terminalMocks.fits.at(-1), 'proposeDimensions').mockReturnValue({ cols: 100, rows: 30 });
    surface.fit(); expect(resize).toHaveBeenCalledExactlyOnceWith(100, 30);
    surface.setVisible?.(false); expect(terminal.options.cursorBlink).toBe(false);
    surface.setVisible?.(true); surface.fit(); expect(terminal.refresh).toHaveBeenCalledTimes(2);
    expect(terminal.options.cursorBlink).toBe(true);
    dimensions.mockRestore(); surface.dispose();
  });
  it('bounds scrollback and consumes clipboard/hyperlink escape handlers without browser effects', () => {
    const surface = createTerminalSurface(() => {}); const terminal = terminalMocks.terminals.at(-1);
    expect(terminal.options.scrollback).toBe(3000); expect(terminal.options.linkHandler.allowNonHttpProtocols).toBe(false);
    expect(terminal.options.allowProposedApi).toBe(true);
    expect(terminal.options.minimumContrastRatio).toBe(1);
    expect(terminal.options.drawBoldTextInBrightColors).toBe(false);
    expect(terminal.osc.get(52)('c;?')).toBe(true); expect(terminal.osc.get(8)(';javascript:alert(1)')).toBe(true);
    expect(() => terminal.options.linkHandler.activate({}, 'javascript:alert(1)')).not.toThrow(); surface.dispose();
  });
});

describe('terminal any-motion selection preservation', () => {
  function openedSurface(clipboard?: Parameters<typeof createTerminalSurface>[2]) {
    const surface = createTerminalSurface(() => {}, undefined, clipboard);
    document.body.append(surface.element); surface.fit();
    const terminal = terminalMocks.terminals.at(-1);
    terminal.modes.mouseTrackingMode = 'any';
    const received = vi.fn();
    const documentReceived = vi.fn();
    terminal.element.addEventListener('mousemove', received);
    document.addEventListener('mousemove', documentReceived);
    return {
      surface, terminal, received, documentReceived,
      move(extra: MouseEventInit = {}) {
        const event = new MouseEvent('mousemove', { bubbles: true, cancelable: true, buttons: 0, ...extra });
        terminal.element.dispatchEvent(event);
        return event;
      },
      dispose() { document.removeEventListener('mousemove', documentReceived); surface.dispose(); },
    };
  }

  it('preserves selection after Shift is released so Ctrl+Shift+C can copy it', () => {
    const copy = vi.fn();
    const fixture = openedSurface({ copy, read: async () => null });
    try {
      fixture.terminal.selection = 'selected text';
      // Model the xterm SGR hover handler that clears selection as user input.
      fixture.received.mockImplementation(() => { fixture.terminal.selection = ''; });
      expect(fixture.move().defaultPrevented).toBe(false);
      expect(fixture.received).not.toHaveBeenCalled();
      expect(fixture.documentReceived).not.toHaveBeenCalled();
      expect(fixture.terminal.getSelection()).toBe('selected text');
      fixture.terminal.keyHandler(new KeyboardEvent('keydown', { code: 'KeyC', ctrlKey: true, shiftKey: true, cancelable: true }));
      expect(copy).toHaveBeenCalledExactlyOnceWith('selected text');
      expect(fixture.terminal.keyHandler(new KeyboardEvent('keydown', { code: 'KeyC', ctrlKey: true }))).toBe(true);
    } finally { fixture.dispose(); }
  });

  it('blocks Shift-hover before a local selection exists, without needing a clipboard bridge', () => {
    const fixture = openedSurface();
    try {
      expect(fixture.move({ shiftKey: true }).defaultPrevented).toBe(false);
      expect(fixture.received).not.toHaveBeenCalled();
      expect(fixture.documentReceived).not.toHaveBeenCalled();
    } finally { fixture.dispose(); }
  });

  it('passes ordinary TUI hover when neither selection nor Shift is present', () => {
    const fixture = openedSurface();
    try {
      expect(fixture.move().defaultPrevented).toBe(false);
      expect(fixture.received).toHaveBeenCalledOnce();
      expect(fixture.documentReceived).toHaveBeenCalledOnce();
    } finally { fixture.dispose(); }
  });

  it.each([1, 2, 4])('passes button-held movement with buttons=%i, including Shift-drag', buttons => {
    const fixture = openedSurface();
    try {
      for (const shiftKey of [false, true]) {
        fixture.terminal.selection = shiftKey ? 'selected text' : '';
        fixture.received.mockClear(); fixture.documentReceived.mockClear();
        expect(fixture.move({ buttons, shiftKey }).defaultPrevented).toBe(false);
        expect(fixture.received).toHaveBeenCalledOnce();
        expect(fixture.documentReceived).toHaveBeenCalledOnce();
      }
    } finally { fixture.dispose(); }
  });

  it.each(['mousedown', 'mouseup'])('leaves ordinary and Shift-selection %s alone', type => {
    const fixture = openedSurface();
    const received = vi.fn(); const bubbled = vi.fn();
    fixture.terminal.element.addEventListener(type, received);
    fixture.surface.element.addEventListener(type, bubbled);
    try {
      for (const shiftKey of [false, true]) {
        fixture.terminal.selection = shiftKey ? 'selected text' : '';
        received.mockClear(); bubbled.mockClear();
        const event = new MouseEvent(type, { bubbles: true, cancelable: true, buttons: type === 'mousedown' ? 1 : 0, shiftKey });
        fixture.terminal.element.dispatchEvent(event);
        expect(received).toHaveBeenCalledOnce();
        expect(bubbled).toHaveBeenCalledOnce();
        expect(event.defaultPrevented).toBe(false);
      }
    } finally { fixture.dispose(); }
  });

  it.each(['none', 'x10', 'vt200', 'drag'])('leaves hover alone in %s mode', mode => {
    const fixture = openedSurface();
    try {
      fixture.terminal.modes.mouseTrackingMode = mode;
      fixture.terminal.selection = 'selected text';
      expect(fixture.move({ shiftKey: true }).defaultPrevented).toBe(false);
      expect(fixture.received).toHaveBeenCalledOnce();
      expect(fixture.documentReceived).toHaveBeenCalledOnce();
    } finally { fixture.dispose(); }
  });

  it('removes the capture listener on disposal', () => {
    const fixture = openedSurface();
    const remove = vi.spyOn(fixture.surface.element, 'removeEventListener');
    fixture.terminal.selection = 'selected text';
    fixture.dispose();
    expect(remove).toHaveBeenCalledWith('mousemove', expect.any(Function), true);
    // Retain the wrapper and dispatch from a child to verify removal, not just detachment.
    const child = document.createElement('div'); fixture.surface.element.append(child);
    const received = vi.fn(); child.addEventListener('mousemove', received);
    child.dispatchEvent(new MouseEvent('mousemove', { bubbles: true, buttons: 0, shiftKey: true }));
    expect(received).toHaveBeenCalledOnce();
  });
});

describe('terminal clipboard shortcuts', () => {
  const key = (code: string, extra: Partial<KeyboardEventInit> = {}, type = 'keydown') => new KeyboardEvent(type, { code, ctrlKey: true, shiftKey: true, cancelable: true, ...extra });
  it('recognizes only Ctrl+Shift+C / Ctrl+Shift+V', () => {
    expect(clipboardShortcut(key('KeyC'))).toBe('copy'); expect(clipboardShortcut(key('KeyV'))).toBe('paste');
    expect(clipboardShortcut(key('KeyC', { shiftKey: false }))).toBeNull();
    expect(clipboardShortcut(key('KeyV', { altKey: true }))).toBeNull();
    expect(clipboardShortcut(key('KeyX'))).toBeNull();
  });
  it('copies the selection and pastes clipboard text without sending control characters to the shell', async () => {
    const sent: string[] = []; const copy = vi.fn(); const read = vi.fn(async () => 'echo hi');
    const surface = createTerminalSurface(data => sent.push(data), undefined, { copy, read }); const terminal = terminalMocks.terminals.at(-1);
    terminal.selection = 'selected text';
    const down = key('KeyC'); expect(terminal.keyHandler(down)).toBe(false); expect(down.defaultPrevented).toBe(true);
    expect(terminal.keyHandler(key('KeyC', {}, 'keyup'))).toBe(false);
    expect(copy).toHaveBeenCalledExactlyOnceWith('selected text');
    terminal.selection = ''; terminal.keyHandler(key('KeyC')); expect(copy).toHaveBeenCalledTimes(1);
    expect(terminal.keyHandler(key('KeyV'))).toBe(false); await flush();
    expect(terminal.paste).toHaveBeenCalledWith('echo hi'); expect(sent).toEqual(['echo hi']);
    expect(terminal.keyHandler(key('KeyC', { shiftKey: false }))).toBe(true);
    surface.dispose();
  });
  it('distinguishes parser replies from typing and clipboard paste during a pending write', async () => {
    const sent: [string, boolean | undefined][] = [];
    const surface = createTerminalSurface((data, user) => sent.push([data, user]), undefined, { copy() {}, read: async () => 'paste' });
    const terminal = terminalMocks.terminals.at(-1);
    let parsed = () => {};
    terminal.write = (_data: string, done: () => void) => { parsed = done; };
    surface.write('output', () => {});
    terminal.input('device reply');
    surface.element.dispatchEvent(new KeyboardEvent('keydown', { key: 'a' }));
    terminal.input('a'); await flush();
    terminal.keyHandler(key('KeyV')); await flush();
    expect(sent).toEqual([['device reply', false], ['a', true], ['paste', true]]);
    parsed(); surface.dispose();
  });
  it('does not paste into a disposed surface after a late clipboard read', async () => {
    const pending = deferred<string | null>();
    const surface = createTerminalSurface(() => {}, undefined, { copy() {}, read: () => pending.promise });
    const terminal = terminalMocks.terminals.at(-1);
    terminal.keyHandler(key('KeyV')); surface.dispose(); pending.resolve('late paste'); await flush();
    expect(terminal.paste).not.toHaveBeenCalled();
  });
  it('routes registry clipboard through the main-process bridge', async () => {
    const fixture = mockApi(); vi.mocked(fixture.api.readClipboardText!).mockResolvedValueOnce(success({ text: 'ls' }));
    const registry = new TerminalRegistry(fixture.api); registry.start();
    registry.mount(tab.id, document.createElement('div'), true); await flush();
    const terminal = terminalMocks.terminals.at(-1); terminal.selection = 'out';
    terminal.keyHandler(key('KeyC')); await flush();
    expect(fixture.api.copyText).toHaveBeenCalledWith({ text: 'out' });
    terminal.keyHandler(key('KeyV')); await flush();
    expect(terminal.paste).toHaveBeenCalledWith('ls');
    registry.dispose();
  });
});
