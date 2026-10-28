import { EventEmitter } from 'node:events';
import { powerMonitor, type BrowserWindow } from 'electron';
import { expect, it, vi } from 'vitest';
import { onSystemQuit } from './system-quit';

vi.mock('electron', async () => ({ powerMonitor: new (await import('node:events')).EventEmitter() }));
const monitor = powerMonitor as unknown as EventEmitter;
const counts = () => ['SIGTERM', 'SIGHUP', 'SIGINT'].map(signal => process.listenerCount(signal));
// Call only the listeners under test; emitting a real signal event would also reach the test runner's handlers.
const added = (signal: NodeJS.Signals, before: NodeJS.SignalsListener[]) => process.listeners(signal).filter(listener => !before.includes(listener));

it.each(['linux', 'darwin'] as const)('quits without a prompt on SIGTERM, SIGHUP and %s shutdown, but not SIGINT', platform => {
  const quit = vi.fn(), window = new EventEmitter() as unknown as BrowserWindow, before = counts();
  const term: NodeJS.SignalsListener[] = process.listeners('SIGTERM'), hup: NodeJS.SignalsListener[] = process.listeners('SIGHUP');
  const dispose = onSystemQuit(window, quit, platform);
  try {
    for (const listener of [...added('SIGTERM', term), ...added('SIGHUP', hup)]) listener('SIGTERM');
    const event = { preventDefault: vi.fn() }; monitor.emit('shutdown', event);
    expect(quit).toHaveBeenCalledTimes(3); expect(event.preventDefault).toHaveBeenCalled();
    expect(process.listenerCount('SIGINT')).toBe(before[2]);
  } finally { dispose(); }
  expect(counts()).toEqual(before); expect(monitor.listenerCount('shutdown')).toBe(0);
});
it('uses the window session end on Windows', () => {
  const quit = vi.fn(), window = new EventEmitter(), before = counts();
  const dispose = onSystemQuit(window as unknown as BrowserWindow, quit, 'win32');
  expect(counts()).toEqual(before);
  window.emit('session-end'); expect(quit).toHaveBeenCalledTimes(1);
  dispose(); expect(window.listenerCount('session-end')).toBe(0);
});
