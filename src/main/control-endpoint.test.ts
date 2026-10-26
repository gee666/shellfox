import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import path from 'node:path';
import { link, mkdir, mkdtemp, readdir, rm, symlink, writeFile } from 'node:fs/promises';
import { z } from 'zod';
import { preparePrivateDirectory, publishPrivateEndpoint, readPrivateEndpoint } from './control-endpoint';

const run = vi.hoisted(() => vi.fn<(...args: unknown[]) => Promise<{ stdout: string; stderr: string }>>());
vi.mock('node:child_process', () => ({
  execFile: Object.assign(vi.fn(), { [Symbol.for('nodejs.util.promisify.custom')]: run }),
}));
const schema = z.object({ secret: z.string() }).strict();
const actualPlatform = process.platform;
let root: string, directory: string, file: string;
beforeEach(async () => {
  run.mockReset().mockResolvedValue({ stdout: '{"ok":true}\n', stderr: '' });
  vi.stubGlobal('process', new Proxy(process, { get: (target, key) => key === 'platform' ? 'win32' : Reflect.get(target, key) }));
  vi.stubEnv('SHELLFOX_RESOURCES_PATH', path.resolve('tmp/permissions resources'));
  await mkdir('tmp', { recursive: true });
  root = await mkdtemp(path.resolve('tmp/control-permissions-'));
  directory = path.join(root, "private ' 界"); file = path.join(directory, 'endpoint.json');
});
afterEach(async () => { vi.unstubAllGlobals(); vi.unstubAllEnvs(); await rm(root, { recursive: true, force: true }); });

it('uses the bundled offline helper with absolute argument arrays for every protect/check', async () => {
  const remove = await publishPrivateEndpoint(root, file, schema, { secret: 'fixture' });
  expect(run.mock.calls.map(call => call[1])).toEqual([
    ['--private-permissions', directory, 'directory', 'protect'],
    ['--private-permissions', directory, 'directory', 'check'],
    ['--private-permissions', expect.any(String), 'file', 'protect'],
  ]);
  const staged = run.mock.calls[2]![1];
  expect(Array.isArray(staged) && path.dirname(String(staged[1]))).toBe(directory);
  for (const call of run.mock.calls) {
    expect(call[0]).toBe(path.resolve('tmp/permissions resources/tailscale/shellfox-tailscale.exe'));
    expect(call[2]).toEqual({ windowsHide: true, timeout: 8000, maxBuffer: 1024 });
  }
  run.mockClear();
  expect(await readPrivateEndpoint(file, schema)).toEqual({ secret: 'fixture' });
  expect(run.mock.calls.map(call => call[1])).toEqual([
    ['--private-permissions', directory, 'directory', 'check'],
    ['--private-permissions', file, 'file', 'check'],
  ]);
  await remove(); expect(await readdir(directory)).toEqual([]);
});

it.each([
  { stdout: '', stderr: '' },
  { stdout: '{}', stderr: '' },
  { stdout: '{"ok":false}', stderr: '' },
  { stdout: '{"ok":true,"extra":"do-not-echo"}', stderr: '' },
  { stdout: '{"ok":true}\n{"ok":true}', stderr: '' },
  { stdout: '{"ok":true}', stderr: 'do-not-echo' },
])('fails closed on incompatible helper replies without echoing output: %j', async result => {
  run.mockResolvedValueOnce(result);
  await expect(preparePrivateDirectory(root, directory)).rejects.toThrow('Unsafe control endpoint permissions or unavailable permissions helper.');
});

it.each(['ENOENT', 'ETIMEDOUT', 'ERR_CHILD_PROCESS_STDIO_MAXBUFFER'])('fails closed on helper %s without exposing process errors', async code => {
  run.mockRejectedValueOnce(Object.assign(new Error('do-not-echo'), { code }));
  try { await preparePrivateDirectory(root, directory); throw new Error('Unexpected success'); }
  catch (error) {
    expect(error).toEqual(new Error('Unsafe control endpoint permissions or unavailable permissions helper.'));
    expect(String(error)).not.toContain('do-not-echo');
  }
});

it('checks an existing directory before any attempt to protect it', async () => {
  await mkdir(directory);
  run.mockRejectedValueOnce(new Error('unsafe existing DACL'));
  await expect(preparePrivateDirectory(root, directory)).rejects.toThrow('Unsafe');
  expect(run.mock.calls.map(call => call[1])).toEqual([['--private-permissions', directory, 'directory', 'check']]);
});

it('does not write credentials and removes the staged file when protection fails', async () => {
  run.mockResolvedValueOnce({ stdout: '{"ok":true}', stderr: '' })
    .mockResolvedValueOnce({ stdout: '{"ok":true}', stderr: '' })
    .mockRejectedValueOnce(new Error('cannot protect staged file'));
  await expect(publishPrivateEndpoint(root, file, schema, { secret: 'fixture' })).rejects.toThrow('Unsafe');
  expect(await readdir(directory)).toEqual([]);
});

it('retains Node hardlink validation before file ACL invocation', async () => {
  await mkdir(directory); await writeFile(file, '{"secret":"fixture"}');
  await link(file, path.join(directory, 'alias.json'));
  await expect(readPrivateEndpoint(file, schema)).rejects.toThrow('Unsafe control endpoint file');
  expect(run.mock.calls.map(call => call[1])).toEqual([['--private-permissions', directory, 'directory', 'check']]);
});

it.skipIf(actualPlatform === 'win32')('retains Node symlink validation before ACL invocation', async () => {
  const target = path.join(root, 'target'); await mkdir(target); await symlink(target, directory);
  await expect(readPrivateEndpoint(file, schema)).rejects.toThrow('Unsafe control endpoint file');
  expect(run).not.toHaveBeenCalled();
});

describe.skipIf(actualPlatform === 'win32')('unchanged Unix permissions', () => {
  it('uses Node modes and never invokes the Windows helper', async () => {
    vi.unstubAllGlobals();
    const remove = await publishPrivateEndpoint(root, file, schema, { secret: 'fixture' });
    expect(await readPrivateEndpoint(file, schema)).toEqual({ secret: 'fixture' });
    await remove(); expect(run).not.toHaveBeenCalled();
  });
});
