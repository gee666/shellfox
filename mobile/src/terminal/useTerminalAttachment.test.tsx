// @vitest-environment jsdom
import React, { type ReactNode } from 'react';
import { act, cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { success } from '../../../src/shared/contracts';
import type { TerminalUploadApi, UploadIdentity } from './upload';
import { useTerminalAttachment } from './useTerminalAttachment';
const native = vi.hoisted(() => ({ pick: vi.fn(), read: vi.fn(), release: vi.fn(), cancelPick: vi.fn(), copy: vi.fn() }));
vi.mock('../files', () => ({ pickedFiles: native }));
vi.mock('uuid', () => ({ v4: () => '50000000-0000-4000-8000-000000000001' }));
vi.mock('@react-native-clipboard/clipboard', () => ({ default: { setString: (text: string) => native.copy(text) } }));
vi.mock('react-native', () => ({
  Alert: { alert: vi.fn() },
  StyleSheet: { create: <T,>(styles: T) => styles },
  View: ({ children }: { children?: ReactNode }) => <div>{children}</div>,
  Text: ({ children }: { children?: ReactNode }) => <span>{children}</span>,
  Pressable: ({ children, accessibilityLabel, disabled, onPress }: { children?: ReactNode; accessibilityLabel: string; disabled: boolean; onPress: () => void }) =>
    <button aria-label={accessibilityLabel} disabled={disabled} onClick={onPress}>{children}</button>,
}));
const identity: UploadIdentity = { tabId: '10000000-0000-4000-8000-000000000001', generation: '20000000-0000-4000-8000-000000000001', controlEpoch: 1 };
const handle = '40000000-0000-4000-8000-000000000001';
const path = 'C:\\Users\\日本語\\upload folder\\photo $test.png';
let api: TerminalUploadApi;
let current: boolean;
const insert = vi.fn();
function Fixture({ contextKey = 'a', enabled = true }: { contextKey?: string; enabled?: boolean }) {
  const attachment = useTerminalAttachment({
    api, contextKey, enabled, getContext: () => current ? { identity, current: () => current } : null, insert,
  });
  return <>{attachment.button}{attachment.panel}</>;
}
function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>(done => { resolve = done; });
  return { promise, resolve };
}
beforeEach(() => {
  vi.resetAllMocks(); current = true;
  native.pick.mockResolvedValue({ handle, fileName: 'photo.png', size: 0 });
  native.release.mockResolvedValue(undefined); native.cancelPick.mockResolvedValue(undefined);
  api = {
    beginTerminalUpload: vi.fn(async () => success({ uploadId: '30000000-0000-4000-8000-000000000001', chunkBytes: 196608 })),
    writeTerminalUpload: vi.fn(),
    finishTerminalUpload: vi.fn(async () => success({ path, fileName: 'photo.png', size: 0 })),
    cancelTerminalUpload: vi.fn(async () => success({ cancelled: true as const })),
  };
});
afterEach(cleanup);
it('shows a compact attachment result; Copy and Insert use the exact raw path without submission', async () => {
  render(<Fixture />);
  fireEvent.click(screen.getByRole('button', { name: 'Attach' }));
  await screen.findByText(path);
  expect(insert).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole('button', { name: 'Copy path' }));
  expect(native.copy).toHaveBeenCalledExactlyOnceWith(path);
  fireEvent.click(screen.getByRole('button', { name: 'Insert path' }));
  expect(insert).toHaveBeenCalledExactlyOnceWith(path);
  expect(insert.mock.calls[0]?.[0]).not.toContain('\n');
  expect(api.writeTerminalUpload).not.toHaveBeenCalled();
  expect(native.release).toHaveBeenCalledExactlyOnceWith(handle);
});
it('cancels the native picker and suppresses a late result', async () => {
  const pending = deferred<{ handle: string; fileName: string; size: number } | null>();
  native.pick.mockReturnValue(pending.promise);
  render(<Fixture />);
  fireEvent.click(screen.getByRole('button', { name: 'Attach' }));
  fireEvent.click(screen.getByRole('button', { name: 'Cancel attachment' }));
  await act(async () => pending.resolve({ handle, fileName: 'late', size: 0 }));
  expect(native.cancelPick).toHaveBeenCalledOnce();
  expect(native.release).toHaveBeenCalledExactlyOnceWith(handle);
  expect(api.beginTerminalUpload).not.toHaveBeenCalled();
  expect(screen.queryByRole('button', { name: 'Insert path' })).toBeNull();
});
it('cleans up on terminal switch, disconnect and unmount', async () => {
  const pending = deferred<null>();
  native.pick.mockReturnValue(pending.promise);
  const view = render(<Fixture />);
  fireEvent.click(screen.getByRole('button', { name: 'Attach' }));
  view.rerender(<Fixture contextKey="new connection" enabled={false} />);
  expect(native.cancelPick).toHaveBeenCalledOnce();
  expect(screen.queryByRole('button', { name: 'Cancel attachment' })).toBeNull();
  await act(async () => pending.resolve(null));
  view.unmount();
  expect(api.beginTerminalUpload).not.toHaveBeenCalled();
});
it('rejects stale path actions even before React processes a changed ownership epoch', async () => {
  render(<Fixture />);
  fireEvent.click(screen.getByRole('button', { name: 'Attach' }));
  await screen.findByText(path);
  current = false;
  fireEvent.click(screen.getByRole('button', { name: 'Insert path' }));
  expect(insert).not.toHaveBeenCalled();
  expect(native.copy).not.toHaveBeenCalled();
  expect(screen.queryByRole('button', { name: 'Insert path' })).toBeNull();
});
it('does not open the picker when the terminal is not attached', () => {
  render(<Fixture enabled={false} />);
  fireEvent.click(screen.getByRole('button', { name: 'Attach' }));
  expect(native.pick).not.toHaveBeenCalled();
});
