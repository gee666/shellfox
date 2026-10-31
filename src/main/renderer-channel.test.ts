import type { BrowserWindow } from 'electron';
import { expect, it, vi } from 'vitest';
import { rendererShowing, sendToRenderer } from './renderer-channel';

const url = 'file:///shellfox/renderer/index.html';
const disposed = () => { throw new Error('Render frame was disposed before WebFrameMain could be accessed'); };
function window(frame: () => { url: string }, send = vi.fn(), destroyed = false) {
  const webContents = { send };
  Object.defineProperty(webContents, 'mainFrame', { get: frame });
  return { isDestroyed: () => destroyed, webContents } as unknown as BrowserWindow;
}

it('sends only to the expected live renderer document', () => {
  const send = vi.fn();
  sendToRenderer(window(() => ({ url }), send), url, 'channel', 1);
  sendToRenderer(window(() => ({ url: 'https://other.invalid' }), send), url, 'channel', 2);
  sendToRenderer(window(() => ({ url }), send, true), url, 'channel', 3);
  expect(send.mock.calls).toEqual([['channel', 1]]);
});
it('treats a disposed render frame as not showing instead of throwing', () => {
  expect(rendererShowing(window(disposed), url)).toBe(false);
  const send = vi.fn(disposed);
  expect(() => sendToRenderer(window(() => ({ url }), send), url, 'channel', 1)).not.toThrow();
});
