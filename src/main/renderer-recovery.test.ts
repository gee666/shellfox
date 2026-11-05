import { EventEmitter } from 'node:events';
import type { BrowserWindow } from 'electron';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { recoverRenderer } from './renderer-recovery';

const dialogs = vi.hoisted(() => ({ showMessageBox: vi.fn() }));
vi.mock('electron', () => ({ dialog: dialogs }));
const rendererUrl = 'file:///shellfox/renderer/index.html';
const retryUrl = 'https://shellfox.invalid/renderer-retry';
beforeEach(() => {
  vi.useFakeTimers(); vi.spyOn(console, 'error').mockImplementation(() => {});
  dialogs.showMessageBox.mockReset().mockResolvedValue({ response: 1 });
});
afterEach(() => { vi.useRealTimers(); vi.restoreAllMocks(); });

function fixture(options: Parameters<typeof recoverRenderer>[2] = {}) {
  const webContents = new EventEmitter();
  let destroyed = false;
  const loadURL = vi.fn().mockResolvedValue(undefined);
  const window = Object.assign(new EventEmitter(), { webContents, loadURL, isDestroyed: () => destroyed });
  recoverRenderer(window as unknown as BrowserWindow, rendererUrl, { delayMs: 10, ...options });
  const gone = (reason = 'crashed') => webContents.emit('render-process-gone', {}, { reason, exitCode: 1 });
  const navigate = (url = retryUrl) => {
    const event = { preventDefault: vi.fn() };
    webContents.emit('will-navigate', event, url);
    return event;
  };
  const destroy = () => { destroyed = true; window.emit('closed'); };
  return { loadURL, gone, navigate, destroy };
}

it('reloads a crashed or killed renderer', () => {
  const { loadURL, gone } = fixture();
  gone('crashed'); vi.advanceTimersByTime(10);
  gone('killed'); vi.advanceTimersByTime(10);
  expect(loadURL.mock.calls).toEqual([[rendererUrl], [rendererUrl]]);
});

it('ignores clean exits and cancels pending recovery when the window closes', () => {
  const { loadURL, gone, destroy } = fixture();
  gone('clean-exit'); vi.advanceTimersByTime(10);
  gone(); destroy(); vi.advanceTimersByTime(10); gone();
  expect(loadURL).not.toHaveBeenCalled();
});

it('shows a script-free recovery page after exhausting automatic retries', () => {
  const { loadURL, gone } = fixture();
  for (let i = 0; i < 4; i++) { gone(); vi.advanceTimersByTime(10); }
  expect(loadURL).toHaveBeenCalledTimes(4);
  expect(loadURL.mock.calls.slice(0, 3)).toEqual([[rendererUrl], [rendererUrl], [rendererUrl]]);
  const page = decodeURIComponent(String(loadURL.mock.calls[3]?.[0]));
  expect(page).toContain(`<a href="${retryUrl}">Retry</a>`);
  expect(page).toContain('Your terminal sessions are still running.');
  expect(page).not.toContain('<script');
});

it('manual retry loads the app, not the recovery page, and resets the crash allowance', () => {
  const { loadURL, gone, navigate } = fixture({ maxReloads: 1 });
  gone(); vi.advanceTimersByTime(10); gone();
  expect(navigate().preventDefault).toHaveBeenCalledOnce();
  expect(loadURL).toHaveBeenLastCalledWith(rendererUrl);
  gone(); vi.advanceTimersByTime(10);
  expect(loadURL).toHaveBeenLastCalledWith(rendererUrl);
  expect(loadURL).toHaveBeenCalledTimes(4);
  gone();
  expect(loadURL.mock.lastCall?.[0]).toMatch(/^data:text\/html/);
});

it('does not handle retry navigation unless the recovery page is showing', () => {
  const { loadURL, gone, navigate } = fixture({ maxReloads: 0 });
  expect(navigate().preventDefault).not.toHaveBeenCalled();
  gone();
  expect(navigate('https://example.com').preventDefault).not.toHaveBeenCalled();
  expect(loadURL).toHaveBeenCalledTimes(1);
});

it('expires old automatic retries before the limit is reached', () => {
  let time = 0;
  const { loadURL, gone } = fixture({ maxReloads: 2, windowMs: 1000, now: () => time });
  gone(); vi.advanceTimersByTime(10);
  time = 1000; gone('oom'); vi.advanceTimersByTime(10);
  gone(); vi.advanceTimersByTime(10);
  expect(loadURL.mock.calls).toEqual([[rendererUrl], [rendererUrl], [rendererUrl]]);
});

it('cancels a pending reload when the crash limit is reached', () => {
  const { loadURL, gone } = fixture({ maxReloads: 1 });
  gone(); gone(); vi.advanceTimersByTime(10);
  expect(loadURL).toHaveBeenCalledTimes(1);
  expect(loadURL.mock.lastCall?.[0]).toMatch(/^data:text\/html/);
});

it('falls back to one native retry dialog if even the recovery document crashes', async () => {
  const { loadURL, gone } = fixture({ maxReloads: 0 });
  dialogs.showMessageBox.mockResolvedValueOnce({ response: 0 });
  gone();
  for (let i = 0; i < 5; i++) gone();
  expect(loadURL).toHaveBeenCalledTimes(1);
  expect(dialogs.showMessageBox).toHaveBeenCalledOnce();
  await Promise.resolve();
  expect(loadURL).toHaveBeenLastCalledWith(rendererUrl);
});

it('does not reload if the native retry dialog is cancelled or the window closes', async () => {
  const cancelled = fixture({ maxReloads: 0 });
  cancelled.gone(); cancelled.gone(); await Promise.resolve(); cancelled.gone();
  expect(cancelled.loadURL).toHaveBeenCalledTimes(1);
  expect(dialogs.showMessageBox).toHaveBeenCalledOnce();
  dialogs.showMessageBox.mockResolvedValueOnce({ response: 0 });
  const closed = fixture({ maxReloads: 0 });
  closed.gone(); closed.gone(); closed.destroy(); await Promise.resolve();
  expect(closed.loadURL).toHaveBeenCalledTimes(1);
});

it('handles rejected recovery navigation without an unhandled rejection', async () => {
  const { loadURL, gone } = fixture({ maxReloads: 0 });
  const error = new Error('Window closed during recovery');
  loadURL.mockRejectedValueOnce(error);
  gone(); await Promise.resolve();
  expect(console.error).toHaveBeenCalledWith('Renderer recovery navigation failed:', error);
});
