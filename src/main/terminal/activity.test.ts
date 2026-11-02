import { afterEach, describe, expect, it, vi } from 'vitest';
import { randomUUID } from 'node:crypto';
import type { TerminalBackendEvent } from '../../shared/contracts';
import { TerminalActivity } from './activity';
import { PtyBackend } from './backend';
import { factoryFixture, launchInput } from './test-fixtures';

afterEach(() => vi.useRealTimers());
describe('terminal activity', () => {
  it('emits transitions only and restarts the 2500ms silence deadline', () => {
    vi.useFakeTimers(); const emit = vi.fn(), activity = new TerminalActivity('tab', emit);
    activity.output('x'); activity.output('x'); expect(emit.mock.calls).toEqual([[{ type: 'activity', tabId: 'tab', busy: true }]]);
    vi.advanceTimersByTime(2499); activity.output('x'); vi.advanceTimersByTime(2499); expect(emit).toHaveBeenCalledTimes(1);
    vi.advanceTimersByTime(1); expect(emit).toHaveBeenLastCalledWith({ type: 'activity', tabId: 'tab', busy: false });
    vi.advanceTimersByTime(10000); expect(emit).toHaveBeenCalledTimes(2);
    activity.output('x'); expect(emit).toHaveBeenLastCalledWith({ type: 'activity', tabId: 'tab', busy: true });
    activity.stop(); activity.stop(); expect(emit).toHaveBeenCalledTimes(4); expect(vi.getTimerCount()).toBe(0);
  });
  it('ignores echo for 250ms after each input without extending a busy deadline', () => {
    vi.useFakeTimers(); const emit = vi.fn(), activity = new TerminalActivity('tab', emit);
    activity.input(); activity.output('x'); vi.advanceTimersByTime(249); activity.output('x'); expect(emit).not.toHaveBeenCalled();
    vi.advanceTimersByTime(1); activity.output('x'); expect(emit).toHaveBeenCalledTimes(1);
    vi.advanceTimersByTime(2400); activity.input(); activity.output('x'); vi.advanceTimersByTime(100);
    expect(emit).toHaveBeenLastCalledWith({ type: 'activity', tabId: 'tab', busy: false });
  });
  it('ignores idle cursor/synchronized-output boilerplate, including split across chunks', () => {
    vi.useFakeTimers(); const emit = vi.fn(), activity = new TerminalActivity('tab', emit);
    activity.output('\x1b[?2026h\x1b[35;1H\x1b[?25l\x1b[?2026l'); activity.output('\x1b[?20'); activity.output('26h\x1b[3'); activity.output('5;1H\x1b[A\x1b[?25h');
    expect(emit).not.toHaveBeenCalled();
    activity.output('\x1b[?2026h\x1b[2'); activity.output('KHi'); expect(emit).toHaveBeenCalledTimes(1);
  });
  it('keeps real text, erase, clear-screen and unknown sequences as activity', () => {
    vi.useFakeTimers();
    for (const data of ['hello', '\x1b[2J', '\x1b[K', '\x1b[?1049h', '\x1b[31m', '\x1b]0;t\x07', '\x1b[?2026h\x1b[1;1Hx']) {
      const emit = vi.fn(), activity = new TerminalActivity('tab', emit); activity.output(data); expect(emit, JSON.stringify(data)).toHaveBeenCalledTimes(1); activity.stop();
    }
  });
  it('does not extend a busy deadline with idle boilerplate', () => {
    vi.useFakeTimers(); const emit = vi.fn(), activity = new TerminalActivity('tab', emit);
    activity.output('work'); vi.advanceTimersByTime(2000); activity.output('\x1b[?2026h\x1b[35;1H\x1b[?25l\x1b[?2026l'); vi.advanceTimersByTime(500);
    expect(emit).toHaveBeenLastCalledWith({ type: 'activity', tabId: 'tab', busy: false });
  });
  it('tracks every owned tab independently without attachment and clears timers on exit/disposal', async () => {
    vi.useFakeTimers(); const f = factoryFixture(), backend = new PtyBackend(f.options), events: TerminalBackendEvent[] = [];
    await backend.initialize(); const first = launchInput(), second = { ...launchInput(), tabId: randomUUID() };
    await backend.launch(first); await backend.launch(second); backend.subscribe(e => events.push(e));
    backend.write({ tabId: first.tabId, generation: first.generation, data: 'x' });
    f.processes[0].output('echo'); f.processes[1].output('working');
    expect(events.filter(e => e.type === 'activity')).toEqual([{ type: 'activity', tabId: second.tabId, busy: true }]);
    vi.advanceTimersByTime(250); f.processes[0].output('working'); f.processes[0].exit(0);
    expect(events.filter(e => e.type === 'activity')).toEqual([
      { type: 'activity', tabId: second.tabId, busy: true },
      { type: 'activity', tabId: first.tabId, busy: true },
      { type: 'activity', tabId: first.tabId, busy: false },
    ]);
    await backend.dispose(); expect(events.filter(e => e.type === 'activity').at(-1)).toEqual({ type: 'activity', tabId: second.tabId, busy: false });
    // The headless screen mirror schedules its own parse timers; only activity timers are under test,
    // so let those fire and prove nothing of the activity tracker is left behind.
    const activity = events.filter(e => e.type === 'activity').length; vi.runAllTimers();
    expect(events.filter(e => e.type === 'activity')).toHaveLength(activity); expect(vi.getTimerCount()).toBe(0);
  });
  it('suppresses synchronous echo emitted inside the native write', async () => {
    vi.useFakeTimers(); const f = factoryFixture(), backend = new PtyBackend(f.options), emit = vi.fn();
    await backend.initialize(); const input = launchInput(); await backend.launch(input); backend.subscribe(emit);
    f.processes[0].write.mockImplementation(() => f.processes[0].output('echo'));
    backend.write({ tabId: input.tabId, generation: input.generation, data: 'x' });
    expect(emit.mock.calls.map(([event]) => event.type)).toEqual(['data']); await backend.dispose();
  });
});
