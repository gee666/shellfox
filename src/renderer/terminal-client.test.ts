// @vitest-environment jsdom
import './terminal-test-mocks';
import { terminalMocks } from './terminal-test-mocks';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { success, failure } from '../shared/contracts';
import type { Result, TerminalAttachmentDto, TerminalDataEvent } from '../shared/contracts';
import { TerminalRegistry, createTerminalSurface, type SurfaceFactory } from './terminal-client';
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
  const values: { output: string; input: (data: string) => void; callbacks: (() => void)[]; dispose: ReturnType<typeof vi.fn>; fit: ReturnType<typeof vi.fn>; focus: ReturnType<typeof vi.fn> }[] = [];
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
    expect(fixture.registry.getState(tab.id).error?.message).toBe('Input not confirmed');
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
  it('reports genuine stream errors and exit state without claiming command success', async () => {
    const fixture = setup(); await flush(); fixture.emitTerminal({ type: 'error', tabId: tab.id, generation: tab.generation, error: { code: 'MONITOR_UNAVAILABLE', message: 'Unavailable evidence', retryable: true } });
    expect(fixture.registry.getState(tab.id).error?.code).toBe('MONITOR_UNAVAILABLE');
    fixture.emitTerminal({ type: 'exit', tabId: tab.id, generation: tab.generation, lastSequence: 0, exitCode: 3, signal: null }); await flush();
    expect(fixture.registry.getState(tab.id)).toMatchObject({ phase: 'closed', exitCode: 3 });
  });
});

describe('xterm safety configuration', () => {
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
