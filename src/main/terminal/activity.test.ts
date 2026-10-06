import { afterEach, describe, expect, it, vi } from 'vitest';
import { randomUUID } from 'node:crypto';
import type { TerminalEvent } from '../../shared/contracts';
import { TerminalActivity } from './activity';
import { PtyBackend } from './backend';
import { factoryFixture, launchInput } from './test-fixtures';

afterEach(() => vi.useRealTimers());
describe('terminal activity', () => {
  it('emits transitions only and restarts the 2500ms silence deadline', () => {
    vi.useFakeTimers(); const emit = vi.fn(), activity = new TerminalActivity('tab', emit);
    activity.output(); activity.output(); expect(emit.mock.calls).toEqual([[{ type: 'activity', tabId: 'tab', busy: true }]]);
    vi.advanceTimersByTime(2499); activity.output(); vi.advanceTimersByTime(2499); expect(emit).toHaveBeenCalledTimes(1);
    vi.advanceTimersByTime(1); expect(emit).toHaveBeenLastCalledWith({ type: 'activity', tabId: 'tab', busy: false });
    vi.advanceTimersByTime(10000); expect(emit).toHaveBeenCalledTimes(2);
    activity.output(); expect(emit).toHaveBeenLastCalledWith({ type: 'activity', tabId: 'tab', busy: true });
    activity.stop(); activity.stop(); expect(emit).toHaveBeenCalledTimes(4); expect(vi.getTimerCount()).toBe(0);
  });
  it('ignores echo for 250ms after each input without extending a busy deadline', () => {
    vi.useFakeTimers(); const emit = vi.fn(), activity = new TerminalActivity('tab', emit);
    activity.input(); activity.output(); vi.advanceTimersByTime(249); activity.output(); expect(emit).not.toHaveBeenCalled();
    vi.advanceTimersByTime(1); activity.output(); expect(emit).toHaveBeenCalledTimes(1);
    vi.advanceTimersByTime(2400); activity.input(); activity.output(); vi.advanceTimersByTime(100);
    expect(emit).toHaveBeenLastCalledWith({ type: 'activity', tabId: 'tab', busy: false });
  });
  it('tracks every owned tab independently without attachment and clears timers on exit/disposal', async () => {
    vi.useFakeTimers(); const f = factoryFixture(), backend = new PtyBackend(f.options), events: TerminalEvent[] = [];
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
    expect(vi.getTimerCount()).toBe(0);
  });
  it('suppresses synchronous echo emitted inside the native write', async () => {
    vi.useFakeTimers(); const f = factoryFixture(), backend = new PtyBackend(f.options), emit = vi.fn();
    await backend.initialize(); const input = launchInput(); await backend.launch(input); backend.subscribe(emit);
    f.processes[0].write.mockImplementation(() => f.processes[0].output('echo'));
    backend.write({ tabId: input.tabId, generation: input.generation, data: 'x' });
    expect(emit.mock.calls.map(([event]) => event.type)).toEqual(['data']); await backend.dispose();
  });
});
