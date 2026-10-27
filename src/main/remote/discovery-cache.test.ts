import { afterEach, expect, it, vi } from 'vitest';
import { mkdir, mkdtemp, readFile, rm, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { DiscoveryCache } from './discovery-cache';
import { RemoteDiscovery } from './discovery';
const roots: string[] = [];
afterEach(async () => { for (const root of roots.splice(0)) await rm(root, { recursive: true, force: true }); });
async function fixture() {
  await mkdir('tmp', { recursive: true }); const directory = await mkdtemp(path.resolve('tmp/discovery-cache-')); roots.push(directory);
  return { directory, file: path.join(directory, 'discovered-hosts.json'), cache: new DiscoveryCache(directory) };
}
const peer = (id: string, online = true) => ({ id, hostname: id, online, addresses: ['100.64.0.1'] });
it('persists only confirmed IDs and loads offline hosts only through the current authenticated peer map', async () => {
  const f = await fixture(), changed = vi.fn();
  const initial = new RemoteDiscovery(async id => id === 'host', changed, Date.now, f.cache);
  initial.update([peer('host'), peer('ordinary')]);
  await vi.waitFor(async () => expect(JSON.parse(await readFile(f.file, 'utf8'))).toEqual({ version: 1, ids: ['host'] }));
  const restarted = new RemoteDiscovery(async () => false, changed, Date.now, new DiscoveryCache(f.directory));
  await restarted.load(); expect(restarted.peers()).toEqual([]);
  restarted.update([peer('host', false), peer('ordinary')]); expect(restarted.peers()).toEqual([peer('host', false)]);
  restarted.update([peer('ordinary')]); expect(restarted.peers()).toEqual([]);
  await vi.waitFor(async () => expect(JSON.parse(await readFile(f.file, 'utf8'))).toEqual({ version: 1, ids: [] }));
});
it('validates and bounds persisted cache data without blocking discovery', async () => {
  const f = await fixture();
  for (const value of ['bad JSON', JSON.stringify({ version: 1, ids: ['bad\npeer'] }), JSON.stringify({ version: 2, ids: ['host'] }), JSON.stringify({ version: 1, ids: ['host'], credential: 'never accepted' }), JSON.stringify({ version: 1, ids: Array.from({ length: 1001 }, (_, i) => 'peer-' + i) })]) {
    await writeFile(f.file, value); expect(await f.cache.load()).toEqual([]);
  }
  await f.cache.save(['host', 'host']); expect(await f.cache.load()).toEqual(['host']);
  expect(() => f.cache.save(['bad\npeer'])).toThrow();
});
it('serializes cache writes so a removed peer cannot be restored by an older write', async () => {
  const f = await fixture(); await Promise.all([f.cache.save(['host']), f.cache.save([])]); expect(await f.cache.load()).toEqual([]);
});
