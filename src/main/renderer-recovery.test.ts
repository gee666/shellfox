import { EventEmitter } from 'node:events';
import type { BrowserWindow } from 'electron';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { recoverRenderer } from './renderer-recovery';

beforeEach(() => { vi.useFakeTimers(); vi.spyOn(console, 'error').mockImplementation(() => {}); });
afterEach(() => { vi.useRealTimers(); vi.restoreAllMocks(); });

function fixture(options: Parameters<typeof recoverRenderer>[1] = {}) {
  const webContents = Object.assign(new EventEmitter(), { reload: vi.fn() });
  let destroyed = false;
  const window = { webContents, isDestroyed: () => destroyed } as unknown as BrowserWindow;
  recoverRenderer(window, { delayMs: 10, ...options });
  const gone = (reason: string) => webContents.emit('render-process-gone', {}, { reason, exitCode: 1 });
  return { reload: webContents.reload, gone, destroy: () => { destroyed = true; } };
}

it('reloads a crashed or killed renderer', () => {
  const { reload, gone } = fixture();
  gone('crashed'); vi.advanceTimersByTime(10);
  gone('killed'); vi.advanceTimersByTime(10);
  expect(reload).toHaveBeenCalledTimes(2);
});

it('ignores clean exits and destroyed windows', () => {
  const { reload, gone, destroy } = fixture();
  gone('clean-exit'); vi.advanceTimersByTime(10);
  gone('crashed'); destroy(); vi.advanceTimersByTime(10);
  expect(reload).not.toHaveBeenCalled();
});

it('stops reloading a crash loop and resumes after the window passes', () => {
  let time = 0;
  const { reload, gone } = fixture({ maxReloads: 2, windowMs: 1000, now: () => time });
  for (let i = 0; i < 4; i++) { gone('crashed'); vi.advanceTimersByTime(10); }
  expect(reload).toHaveBeenCalledTimes(2);
  time = 1000; gone('oom'); vi.advanceTimersByTime(10);
  expect(reload).toHaveBeenCalledTimes(3);
});
