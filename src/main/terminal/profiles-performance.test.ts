import { afterEach, expect, it, vi } from 'vitest';
import { discoverProfiles } from './profiles';
import { PIDFD_READY } from './pidfd-preflight';

const flush = () => new Promise<void>(resolve => setImmediate(resolve));
function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>(done => { resolve = done; });
  return { promise, resolve };
}
afterEach(() => vi.useRealTimers());

it('refills four WSL preflight workers without waiting for a slow batch and preserves profile order', async () => {
  const distros = Array.from({ length: 8 }, (_, i) => `Distro${i}`);
  const pending = distros.map(() => deferred<Buffer>());
  const started: string[] = [];
  let live = 0, peak = 0;
  const discovery = discoverProfiles({ platform: 'win32', env: {}, exists: async file => file.endsWith('wsl.exe'),
    run: async (_file, args) => {
      if (args.includes('--quiet')) return Buffer.from(distros.join('\n'));
      const distro = args[1]!, index = distros.indexOf(distro);
      started.push(distro); peak = Math.max(peak, ++live);
      try { return await pending[index]!.promise; } finally { live--; }
    } });
  try {
    await flush(); expect(started).toEqual(distros.slice(0, 4));
    // Distro0 is still booting. Each free slot must take the next queued distro.
    for (let i = 1; i <= 4; i++) { pending[i]!.resolve(Buffer.from(PIDFD_READY)); await flush(); }
    expect(started).toEqual(distros); expect(peak).toBe(4);
  } finally { pending.forEach(p => p.resolve(Buffer.from(PIDFD_READY))); }
  expect((await discovery).profiles.map(p => p.id)).toEqual(distros.map(d => `wsl:${d}`));
});

it('overlaps administrator capability with guest discovery instead of adding their startup delays', async () => {
  vi.useFakeTimers();
  const sleep = (ms: number) => new Promise<void>(resolve => setTimeout(resolve, ms));
  const start = Date.now();
  const discovery = discoverProfiles({ platform: 'win32', env: {}, exists: async () => true,
    administratorCapability: async () => { await sleep(5000); return { available: true, reason: null }; },
    run: async (_file, args) => {
      if (args.includes('--quiet')) return Buffer.from('Slow\nFast\n');
      await sleep(args[1] === 'Slow' ? 8000 : 100);
      return Buffer.from(args[1] === 'Slow' ? 'incompatible' : PIDFD_READY);
    } });
  await vi.runAllTimersAsync();
  const result = await discovery;
  expect(Date.now() - start).toBe(8000);
  expect(result.defaultProfileId).toBe('pwsh');
  expect(result.profiles.map(p => p.id)).toEqual(['pwsh', 'windows-powershell', 'administrator-powershell', 'wsl:Slow', 'wsl:Fast']);
  expect(result.profiles.slice(-2).map(p => [p.available, p.canTerminateDescendants])).toEqual([[false, false], [true, true]]);
});

it('drains guest probes before propagating an unexpected local discovery failure', async () => {
  const guest = deferred<Buffer>(), error = new Error('helper failure');
  let finished = false;
  const discovery = discoverProfiles({ platform: 'win32', env: {}, exists: async () => true,
    administratorCapability: async () => { throw error; },
    run: async (_file, args) => args.includes('--quiet') ? Buffer.from('Ubuntu\n') : guest.promise,
  }).then(() => { finished = true; return null; }, failure => { finished = true; return failure; });
  try { await flush(); expect(finished).toBe(false); }
  finally { guest.resolve(Buffer.from(PIDFD_READY)); }
  expect(await discovery).toBe(error);
});
