import { createHash } from 'node:crypto';
import { mkdir, mkdtemp, readFile, readdir, rm, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
const mocks = vi.hoisted(() => ({ rename: vi.fn<(source: string, destination: string) => Promise<void>>() }));
vi.mock('node:fs/promises', async original => ({ ...await original<typeof import('node:fs/promises')>(), rename: mocks.rename }));
import { pinnedLegacyHelper, prepareWindowsHelper, windowsHelperPath } from './stable-helper';

let directory: string;
const hash = (bytes: string) => createHash('sha256').update(bytes).digest('hex');
beforeEach(async () => {
  await mkdir(path.resolve('tmp'), { recursive: true });
  directory = await mkdtemp(path.resolve('tmp/stable-helper-unit-'));
  const actual = await vi.importActual<typeof import('node:fs/promises')>('node:fs/promises');
  mocks.rename.mockReset().mockImplementation(actual.rename);
});
afterEach(async () => { await rm(directory, { recursive: true, force: true }); });
async function candidate(version: string, bytes: string) {
  const file = path.join(directory, 'app-' + version, 'resources/tailscale/shellfox-tailscale.exe');
  await mkdir(path.dirname(file), { recursive: true }); await writeFile(file, bytes);
  return file;
}
function prepare(file: string, bytes: string) {
  return prepareWindowsHelper({ candidate: file, expectedSha256: hash(bytes), localAppData: path.join(directory, 'local'), arch: 'x64' });
}

it('versioned candidates A and B publish to the exact same network execution path', async () => {
  const a = await candidate('A', 'A'), b = await candidate('B', 'B');
  const launchA = await prepare(a, 'A'), launchB = await prepare(b, 'B');
  expect(launchA.executable).toBe(launchB.executable);
  expect(launchB.executable).toBe(path.join(directory, 'local/Shellfox/helpers/win32-x64/shellfox-tailscale.exe'));
  expect(launchB.args).toEqual(['--expected-sha256', hash('B')]);
  expect(mocks.rename.mock.calls.map(call => call[1])).toEqual([launchA.executable, launchB.executable]);
  expect(mocks.rename.mock.calls.every(call => call[0] !== a && call[0] !== b && call[0].endsWith('.tmp'))).toBe(true);
  expect(await readFile(a, 'utf8')).toBe('A');
  expect(await readFile(b, 'utf8')).toBe('B');
  expect(await readFile(launchB.executable, 'utf8')).toBe('B');
  expect(await readdir(path.dirname(launchB.executable))).toEqual(['shellfox-tailscale.exe']);
});

it('reuses matching stable bytes without staging or rewriting the helper', async () => {
  const a = await candidate('A', 'same'), b = await candidate('B', 'same');
  const launch = await prepare(a, 'same');
  mocks.rename.mockClear();
  expect(await prepare(b, 'same')).toEqual(launch);
  expect(await prepare(path.join(directory, 'no-candidate'), 'same')).toEqual(launch);
  expect(mocks.rename).not.toHaveBeenCalled();
});

it('busy publication fails without overwriting the helper or returning a versioned fallback', async () => {
  const a = await candidate('A', 'A'), b = await candidate('B', 'B');
  const original = await prepare(a, 'A');
  mocks.rename.mockRejectedValue(new Error('Sharing violation'));
  await expect(prepare(b, 'B')).rejects.toThrow('no alternate executable will be launched');
  expect(await readFile(original.executable, 'utf8')).toBe('A');
  expect(await readFile(b, 'utf8')).toBe('B');
  expect(await prepare(a, 'A')).toEqual(original);
  expect(await readdir(path.dirname(original.executable))).toEqual(['shellfox-tailscale.exe']);
});

it('rejects a candidate whose bytes differ from the external build anchor before staging', async () => {
  const file = await candidate('B', 'tampered');
  await expect(prepare(file, 'trusted')).rejects.toThrow('SHA256 mismatch');
  expect(mocks.rename).not.toHaveBeenCalled();
});

it('rehashes the staged bytes before publishing', async () => {
  const file = await candidate('B', 'B');
  const fs = await import('node:fs/promises');
  const copy = vi.spyOn(fs, 'copyFile').mockImplementation(async (_source, destination) => { await writeFile(destination, 'changed'); });
  try {
    await expect(prepare(file, 'B')).rejects.toThrow('Staged Windows helper SHA256 mismatch');
    expect(mocks.rename).not.toHaveBeenCalled();
  } finally { copy.mockRestore(); }
});

it('never removes a stage name it did not create exclusively', async () => {
  const file = await candidate('B', 'B');
  const fs = await import('node:fs/promises');
  let collision: string | undefined;
  const copy = vi.spyOn(fs, 'copyFile').mockImplementation(async (_source, destination) => {
    collision = destination.toString();
    await writeFile(destination, 'preexisting');
    throw Object.assign(new Error('Exists'), { code: 'EEXIST' });
  });
  try {
    await expect(prepare(file, 'B')).rejects.toThrow('Exists');
    expect(collision).toBeDefined();
    if (collision) expect(await readFile(collision, 'utf8')).toBe('preexisting');
    expect(mocks.rename).not.toHaveBeenCalled();
  } finally { copy.mockRestore(); }
});

it('checks final bytes after publication rather than trusting a successful rename', async () => {
  const file = await candidate('B', 'B');
  mocks.rename.mockImplementation(async (_source, target) => { await writeFile(target, 'another version'); });
  await expect(prepare(file, 'B')).rejects.toThrow('changed during publication');
});

it('reuses matching bytes published concurrently without replacing them', async () => {
  const file = await candidate('B', 'B');
  const fs = await import('node:fs/promises');
  const actual = await vi.importActual<typeof import('node:fs/promises')>('node:fs/promises');
  const copy = vi.spyOn(fs, 'copyFile').mockImplementation(async (source, destination, mode) => {
    await actual.copyFile(source, destination, mode);
    await actual.copyFile(file, windowsHelperPath(path.join(directory, 'local'), 'x64'));
  });
  try { await prepare(file, 'B'); expect(mocks.rename).not.toHaveBeenCalled(); }
  finally { copy.mockRestore(); }
});

it('requires an external digest and an absolute per-user architecture-specific directory', async () => {
  const file = await candidate('A', 'A');
  await expect(prepareWindowsHelper({ candidate: file, expectedSha256: '', localAppData: directory, arch: 'x64' })).rejects.toThrow('trusted build SHA256');
  expect(() => windowsHelperPath('relative', 'x64')).toThrow('absolute');
  expect(() => windowsHelperPath(directory, 'ia32')).toThrow('architecture');
  expect(windowsHelperPath(directory, 'arm64')).toContain('win32-arm64');
  expect(mocks.rename).not.toHaveBeenCalled();
});

it('keeps an explicitly pinned legacy helper at its original path without flags or publication', async () => {
  const file = await candidate('legacy', 'old bytes');
  expect(await pinnedLegacyHelper(file, hash('old bytes'))).toEqual({ executable: file, args: [] });
  expect(await readFile(file, 'utf8')).toBe('old bytes');
  expect(mocks.rename).not.toHaveBeenCalled();
  await expect(pinnedLegacyHelper(file, hash('different'))).rejects.toThrow('SHA256 mismatch');
  await expect(pinnedLegacyHelper(file, '')).rejects.toThrow('trusted build SHA256');
});
