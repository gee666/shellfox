import { beforeEach, expect, it, vi } from 'vitest';
const native = vi.hoisted(() => ({ begin: vi.fn(), write: vi.fn(), finish: vi.fn(), cancel: vi.fn() }));
vi.mock('react-native', () => ({ NativeModules: { ShellfoxDownloads: native } }));
import { downloads } from './downloads';
import { isAttachmentPickerOpen } from './attachment-picker-lifecycle';
import { deferred } from './client/test-fixtures';
const token = '10000000-0000-4000-8000-000000000001';
beforeEach(() => { vi.resetAllMocks(); native.cancel.mockResolvedValue(null); });
it('validates native download handles and supports an empty file lifecycle', async () => {
  native.begin.mockResolvedValue({ handle: token }); native.finish.mockResolvedValue({ fileName: 'empty.bin', location: 'Downloads' });
  expect(await downloads.begin(token, 'empty.bin', 0, 'application/octet-stream')).toEqual({ handle: token });
  expect(await downloads.finish(token)).toEqual({ fileName: 'empty.bin', location: 'Downloads' });
  expect(native.write).not.toHaveBeenCalled(); await downloads.cancel(token);
});
it('keeps native save dialogs outside transport deadlines and protects background lifecycle while open', async () => {
  const exported = deferred<unknown>(); native.finish.mockReturnValue(exported.promise);
  const waiting = downloads.finish(token); expect(isAttachmentPickerOpen()).toBe(true);
  exported.resolve(null); expect(await waiting).toBeNull(); expect(isAttachmentPickerOpen()).toBe(false);
});
it('writes bounded base64 chunks and rejects paths, invalid metadata and oversized chunks', async () => {
  native.write.mockResolvedValue({ offset: 1 });
  expect(await downloads.write(token, 0, 'YQ==')).toEqual({ offset: 1 });
  await expect(downloads.begin(token, '../outside', 1, 'text/plain')).rejects.toThrow();
  await expect(downloads.begin(token, 'a', -1, 'text/plain')).rejects.toThrow();
  await expect(downloads.write('/sdcard/Download/a', 0, 'YQ==')).rejects.toThrow();
  await expect(downloads.write(token, 0, 'a'.repeat(262148))).rejects.toThrow();
  expect(native.begin).not.toHaveBeenCalled(); expect(native.write).toHaveBeenCalledOnce();
});
