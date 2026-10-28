import { beforeEach, expect, it, vi } from 'vitest';
const native = vi.hoisted(() => ({ pick: vi.fn(), read: vi.fn(), release: vi.fn(), cancelPick: vi.fn() }));
vi.mock('react-native', () => ({ NativeModules: { ShellfoxFiles: native } }));
import { MAX_FILE_CHUNK_BYTES, MAX_PICKED_FILE_BYTES, pickedFiles } from './files';
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
it('keeps native cancellation null and errors distinct', async () => {
  native.pick.mockResolvedValueOnce(null).mockRejectedValueOnce(new Error('Provider offline'));
  expect(await pickedFiles.pick(token)).toBeNull();
  await expect(pickedFiles.pick(token)).rejects.toThrow('Provider offline');
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
