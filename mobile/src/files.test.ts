import { beforeEach, expect, it, vi } from 'vitest';
const native = vi.hoisted(() => ({ pick: vi.fn(), pickForTransfer: vi.fn(), read: vi.fn(), release: vi.fn(), cancelPick: vi.fn() }));
vi.mock('react-native', () => ({ NativeModules: { ShellfoxFiles: native } }));
import { MAX_FILE_CHUNK_BYTES, MAX_PICKED_FILE_BYTES, pickedFiles } from './files';
import { isAttachmentPickerOpen } from './attachment-picker-lifecycle';
import { deferred } from './client/test-fixtures';
const token = '10000000-0000-4000-8000-000000000001';
const handle = '20000000-0000-4000-8000-000000000001';
beforeEach(() => { vi.resetAllMocks(); native.release.mockResolvedValue(null); native.cancelPick.mockResolvedValue(null); });
it('accepts only native scoped metadata, including empty files and Unicode names', async () => {
  const file = { handle, fileName: '日本語 photo.png', size: 0 };
  native.pick.mockResolvedValue(file);
  expect(await pickedFiles.pick(token)).toEqual(file);
  expect(native.pick).toHaveBeenCalledExactlyOnceWith(token);
  await pickedFiles.release(handle);
  await pickedFiles.cancelPick(token);
  expect(native.release).toHaveBeenCalledExactlyOnceWith(handle);
  expect(native.cancelPick).toHaveBeenCalledExactlyOnceWith(token);
});
it('opts in to transfer-sized files without raising the terminal attachment limit', async () => {
  const file = { handle, fileName: 'large.bin', size: 65 * 1024 * 1024 };
  native.pick.mockResolvedValue(file); native.pickForTransfer.mockResolvedValue(file);
  await expect(pickedFiles.pick(token)).rejects.toThrow('picker limit');
  expect(await pickedFiles.pick(token, MAX_PICKED_FILE_BYTES)).toEqual(file);
  expect(native.pickForTransfer).toHaveBeenCalledExactlyOnceWith(token, MAX_PICKED_FILE_BYTES);
});
it('keeps native cancellation null and errors distinct', async () => {
  native.pick.mockResolvedValueOnce(null).mockRejectedValueOnce(new Error('Provider offline'));
  expect(await pickedFiles.pick(token)).toBeNull();
  await expect(pickedFiles.pick(token)).rejects.toThrow('Provider offline');
});
it.each(['selected', 'cancelled', 'failed'] as const)('guards picker background transitions until the native %s result settles', async outcome => {
  const pending = deferred<unknown>();
  native.pick.mockImplementation(() => {
    expect(isAttachmentPickerOpen()).toBe(true);
    return pending.promise;
  });
  const picking = pickedFiles.pick(token);
  expect(isAttachmentPickerOpen()).toBe(true);
  if (outcome === 'failed') {
    pending.reject(new Error('Provider offline'));
    await expect(picking).rejects.toThrow('Provider offline');
  } else {
    const value = outcome === 'selected' ? { handle, fileName: 'file', size: 0 } : null;
    pending.resolve(value);
    expect(await picking).toEqual(value);
  }
  expect(isAttachmentPickerOpen()).toBe(false);
});
it('clears the picker guard when native validation fails before presenting', async () => {
  await expect(pickedFiles.pick('not-a-token')).rejects.toThrow();
  expect(isAttachmentPickerOpen()).toBe(false);
  expect(native.pick).not.toHaveBeenCalled();
});
it.each([
  { handle: '/sdcard/Download/file', fileName: 'file', size: 1 },
  { handle, fileName: 'file', size: MAX_PICKED_FILE_BYTES + 1 },
  { handle, fileName: 'file', size: -1 },
  { handle, fileName: 'file', size: 0.5 },
  { handle, fileName: '', size: 1 },
  { handle, fileName: 'bad\nname', size: 1 },
  { handle, fileName: 'file', size: 1, path: '/native/path' },
])('rejects unsafe or malformed native picker metadata %#', async value => {
  native.pick.mockResolvedValue(value);
  await expect(pickedFiles.pick(token)).rejects.toThrow();
});
it('reads bounded binary bytes as canonical base64 without decoding the whole file', async () => {
  const data = Buffer.from([0, 255, 128, 13, 10]).toString('base64');
  native.read.mockResolvedValue({ data, bytes: 5 });
  expect(await pickedFiles.read(handle, 196608, 5)).toEqual({ data, bytes: 5 });
  expect(native.read).toHaveBeenCalledExactlyOnceWith(handle, 196608, 5);
});
it.each([
  { data: 'AB==', bytes: 1 }, { data: 'AAB=', bytes: 2 },
  { data: 'AA==\n', bytes: 1 }, { data: '____', bytes: 3 },
  { data: '', bytes: 0 }, { data: 'AA==', bytes: 2 },
  { data: 'AAA=', bytes: 2, path: '/native/path' },
])('rejects truncated, noncanonical or malformed chunks %#', async value => {
  native.read.mockResolvedValue(value);
  await expect(pickedFiles.read(handle, 0, 2)).rejects.toThrow();
});
it('rejects arbitrary paths and unbounded reads before native access', async () => {
  await expect(pickedFiles.read('/etc/passwd', 0, 1)).rejects.toThrow();
  await expect(pickedFiles.read(handle, -1, 1)).rejects.toThrow();
  await expect(pickedFiles.read(handle, 0, 0)).rejects.toThrow();
  await expect(pickedFiles.read(handle, 0, MAX_FILE_CHUNK_BYTES + 1)).rejects.toThrow();
  expect(native.read).not.toHaveBeenCalled();
});
