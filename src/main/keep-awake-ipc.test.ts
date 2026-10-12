import { beforeEach, expect, it, vi } from 'vitest';
import type { BrowserWindow, IpcMainInvokeEvent } from 'electron';
import type { SessionService } from './service';
const state = vi.hoisted(() => ({ handler: undefined as undefined | ((event: unknown, request: unknown) => Promise<unknown>), openExternal: vi.fn(async (_url: string) => {}) }));
vi.mock('electron', () => ({
  ipcMain: { handle: (_channel: string, fn: typeof state.handler) => { state.handler = fn; }, removeHandler: () => {} },
  shell: { openExternal: state.openExternal },
}));
import { installIpc } from './ipc';
import { KeepAwake } from './keep-awake';
const url = 'file:///app/index.html';
function fixture() {
  const frame = { url }, contents = { mainFrame: frame, send: vi.fn() };
  const window = { webContents: contents, isDestroyed: () => false, isFocused: () => true } as unknown as BrowserWindow;
  const event = { sender: contents, senderFrame: frame } as unknown as IpcMainInvokeEvent;
  const service = { backend: {}, subscribe: vi.fn(() => () => {}) } as unknown as SessionService;
  return { window, event, service, contents, frame };
}
const call = (event: unknown, method: string, payload: unknown) => state.handler!(event, { version: 1, method, payload });
beforeEach(() => { state.handler = undefined; state.openExternal.mockClear(); });
it('toggles keep awake for the trusted renderer', async () => {
  const { window, event, service } = fixture(), power = { start: vi.fn(() => 3), stop: vi.fn() };
  installIpc(window, url, service, undefined, undefined, undefined, new KeepAwake(power, { platform: 'freebsd' as NodeJS.Platform }));
  expect(await call(event, 'getKeepAwake', {})).toEqual({ ok: true, value: { active: false } });
  expect(await call(event, 'setKeepAwake', { active: true })).toEqual({ ok: true, value: { active: true } });
  expect(await call(event, 'getKeepAwake', {})).toEqual({ ok: true, value: { active: true } });
  expect(await call(event, 'setKeepAwake', { active: 'yes' })).toMatchObject({ ok: false, error: { code: 'VALIDATION' } });
  expect(await call({ ...event, sender: {} }, 'setKeepAwake', { active: false })).toMatchObject({ ok: false, error: { code: 'AUTH_FAILED' } });
  expect(await call(event, 'setKeepAwake', { active: false })).toEqual({ ok: true, value: { active: false } });
  expect(power.stop).toHaveBeenCalledExactlyOnceWith(3);
});
it('forwards external model toggles on the dedicated event channel and unsubscribes on teardown', async () => {
  const { window, event, service, contents } = fixture();
  const keep = new KeepAwake({ start: () => 3, stop() {} }, { platform: 'freebsd' as NodeJS.Platform });
  const uninstall = installIpc(window, url, service, undefined, undefined, undefined, keep);
  keep.set(true); // Same path used by the external CLI control server.
  expect(contents.send).toHaveBeenCalledExactlyOnceWith('manager:keep-awake', { active: true });
  keep.set(true); expect(contents.send).toHaveBeenCalledOnce();
  await call(event, 'setKeepAwake', { active: false });
  expect(contents.send).toHaveBeenLastCalledWith('manager:keep-awake', { active: false });
  expect(contents.send).toHaveBeenCalledTimes(2);
  expect(service.subscribe).toHaveBeenCalledOnce();
  uninstall(); keep.set(true); expect(contents.send).toHaveBeenCalledTimes(2); keep.dispose();
});
it('does not send keep-awake events to navigated or destroyed windows', () => {
  const { window, service, contents, frame } = fixture();
  const keep = new KeepAwake({ start: () => 3, stop() {} }, { platform: 'freebsd' as NodeJS.Platform });
  const uninstall = installIpc(window, url, service, undefined, undefined, undefined, keep);
  frame.url = 'https://untrusted.example'; keep.set(true); expect(contents.send).not.toHaveBeenCalled();
  frame.url = url; window.isDestroyed = () => true; keep.set(false); expect(contents.send).not.toHaveBeenCalled();
  uninstall(); keep.dispose();
});
it('is unsupported without a keep-awake controller', async () => {
  const { window, event, service } = fixture(); installIpc(window, url, service);
  expect(await call(event, 'setKeepAwake', { active: true })).toMatchObject({ ok: false, error: { code: 'UNSUPPORTED' } });
});
it('opens only http(s) links in the system browser', async () => {
  const { window, event, service } = fixture(); installIpc(window, url, service);
  expect(await call(event, 'openExternal', { url: 'https://example.com/a b' })).toEqual({ ok: true, value: { opened: true } });
  expect(state.openExternal).toHaveBeenCalledExactlyOnceWith('https://example.com/a%20b');
  for (const bad of ['javascript:alert(1)', 'file:///etc/passwd', 'notaurl', 'https://' + 'a'.repeat(2050)]) {
    expect(await call(event, 'openExternal', { url: bad })).toMatchObject({ ok: false, error: { code: 'VALIDATION' } });
  }
  expect(state.openExternal).toHaveBeenCalledOnce();
});
