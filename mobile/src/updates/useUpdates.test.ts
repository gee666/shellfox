// @vitest-environment jsdom
import { act, cleanup, renderHook, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const native = vi.hoisted(() => ({ platform: { OS: 'android' }, state: { currentState: 'active' }, subscribe: vi.fn(), remove: vi.fn() }));
const client = vi.hoisted(() => ({ check: vi.fn(), download: vi.fn() }));
vi.mock('react-native', () => ({ Platform: native.platform, AppState: { ...native.state, addEventListener: native.subscribe } }));
vi.mock('./native', () => ({ updates: client }));
import { updateConstraint, useUpdates } from './useUpdates';

beforeEach(() => {
  vi.resetAllMocks(); native.platform.OS = 'android';
  native.subscribe.mockReturnValue({ remove: native.remove });
  client.check.mockResolvedValue({ version: '0.9.8' });
  client.download.mockResolvedValue({ version: '0.9.8' });
});
afterEach(() => { cleanup(); vi.useRealTimers(); });

describe('mobile automatic update policy', () => {
  it('waits for persisted preferences before downloading with default-on settings', async () => {
    const { result, rerender } = renderHook(({ loaded, enabled }) => useUpdates(loaded, enabled), { initialProps: { loaded: false, enabled: true } });
    expect(client.check).not.toHaveBeenCalled(); expect(client.download).not.toHaveBeenCalled();
    rerender({ loaded: true, enabled: true });
    await waitFor(() => expect(result.current.message).toBe('Update downloaded. Confirm installation when ready.'));
    expect(client.download).toHaveBeenCalledTimes(1);
  });
  it('keeps the update notice but does not download when disabled', async () => {
    const { result } = renderHook(() => useUpdates(true, false));
    await waitFor(() => expect(result.current.version).toBe('0.9.8'));
    expect(client.download).not.toHaveBeenCalled();
  });
  it('does not download if disabled during the release check', async () => {
    let release!: (value: { version: string }) => void;
    client.check.mockImplementation(() => new Promise(resolve => { release = resolve; }));
    const { rerender } = renderHook(({ enabled }) => useUpdates(true, enabled), { initialProps: { enabled: true } });
    rerender({ enabled: false });
    await act(async () => { release({ version: '0.9.8' }); });
    expect(client.download).not.toHaveBeenCalled();
  });
  it('does not run APK checks or downloads on iOS and explains the restriction', () => {
    native.platform.OS = 'ios';
    renderHook(() => useUpdates(true, true));
    expect(client.check).not.toHaveBeenCalled(); expect(client.download).not.toHaveBeenCalled();
    expect(updateConstraint('ios')).toContain('cannot download or install');
    expect(updateConstraint('android')).toContain('cannot install silently or on quit');
  });
  it('leaves a manual retry available after a failed download', async () => {
    client.download.mockRejectedValue(new Error('offline'));
    const { result } = renderHook(() => useUpdates(true, true));
    await waitFor(() => expect(result.current.message).toContain('manual update buttons'));
    expect(result.current.version).toBe('0.9.8');
  });
  it('checks again on foreground and cleans up its subscription', async () => {
    const { result, unmount } = renderHook(() => useUpdates(true, false));
    await waitFor(() => expect(result.current.version).toBe('0.9.8'));
    await act(async () => { native.subscribe.mock.calls[0]![1]('active'); });
    expect(client.check).toHaveBeenCalledTimes(2);
    unmount(); expect(native.remove).toHaveBeenCalledTimes(1);
  });
});
