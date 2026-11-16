import { EventEmitter } from 'node:events';
import { PassThrough } from 'node:stream';
import { afterEach, expect, it, vi } from 'vitest';
import { z } from 'zod';
const mocks = vi.hoisted(() => ({ spawn: vi.fn() }));
vi.mock('node:child_process', () => ({ spawn: mocks.spawn }));
import { RemoteHelper } from './helper';

afterEach(() => { vi.unstubAllGlobals(); vi.clearAllMocks(); });

it.each([
  { platform: 'win32', stableNetworkPath: undefined, args: ['--network-runtime'] },
  { platform: 'win32', stableNetworkPath: undefined, runtimeChannel: 'testing' as const, args: ['--network-runtime', 'testing'] },
  { platform: 'win32', stableNetworkPath: false, runtimeChannel: 'testing' as const, args: [] },
  { platform: 'linux', stableNetworkPath: undefined, args: [] },
  { platform: 'darwin', stableNetworkPath: undefined, args: [] },
])('launches $platform with stableNetworkPath=$stableNetworkPath', async ({ platform, stableNetworkPath, runtimeChannel, args }) => {
  vi.stubGlobal('process', new Proxy(process, { get(target, key) {
    return key === 'platform' ? platform : Reflect.get(target, key);
  } }));
  const child = Object.assign(new EventEmitter(), { stdin: new PassThrough(), stdout: new PassThrough(), stderr: new PassThrough(), kill: vi.fn() });
  mocks.spawn.mockReturnValue(child);
  child.stdin.on('data', chunk => {
    const request = z.object({ id: z.string() }).parse(JSON.parse(chunk.toString()));
    child.stdout.write(JSON.stringify({ id: request.id, ok: true, value: {} }) + '\n');
  });
  const helper = new RemoteHelper('bundled-helper', { stableNetworkPath, runtimeChannel });
  try {
    await helper.command('start', {});
    expect(mocks.spawn).toHaveBeenCalledWith('bundled-helper', args, { stdio: ['pipe', 'pipe', 'pipe'], windowsHide: true });
  } finally { await helper.dispose(); }
});
