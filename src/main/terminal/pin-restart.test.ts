import { describe, it, expect } from 'vitest';
import { randomUUID } from 'node:crypto';
import type { Result, SessionDto } from '../../shared/contracts';
import { snapshotSchema } from '../../shared/schemas';
import { PtyBackend } from './backend';
import { EmbeddedSessionService } from './service';
import { MemoryRepository, factoryFixture } from './test-fixtures';

const value = <T>(r: Result<T>): T => { if (!r.ok) throw new Error(r.error.code + ': ' + r.error.message); return r.value; };
async function start(repository: MemoryRepository) {
  const f = factoryFixture();
  const service = new EmbeddedSessionService(repository, new PtyBackend(f.options), () => ({ setWatch: () => {}, dispose: () => {} }));
  await service.initialize();
  return { ...f, service };
}
const create = async (service: EmbeddedSessionService, title: string) => value(await service.createSession({ cwd: '/work/' + title, title, requestId: randomUUID() }));
const live = (service: EmbeddedSessionService) => value(service.getSnapshot()).sessions;
const titles = (service: EmbeddedSessionService) => live(service).map(s => s.title);
const spawnedEnv = (f: ReturnType<typeof factoryFixture>, call: number) => ((f.factory as unknown as { mock: { calls: unknown[][] } }).mock.calls[call]![2] as { env: Record<string, string> }).env;

describe('pinned sessions', () => {
  it('keeps pinned sessions on top in pin order, and unpinning restores the normal order', async () => {
    const repository = new MemoryRepository(), { service } = await start(repository);
    const a = await create(service, 'a'), b = await create(service, 'b'), c = await create(service, 'c');
    expect(titles(service)).toEqual(['a', 'b', 'c']);
    const pinnedC = value(await service.setSessionPinned({ sessionId: c.id, pinned: true }));
    expect(pinnedC.pinnedAt).toEqual(expect.any(String));
    expect(titles(service)).toEqual(['c', 'a', 'b']);
    await new Promise(resolve => setTimeout(resolve, 5));
    await service.setSessionPinned({ sessionId: b.id, pinned: true });
    expect(titles(service)).toEqual(['c', 'b', 'a']);
    // Pinning again keeps the original pin time (no reordering).
    await service.setSessionPinned({ sessionId: c.id, pinned: true });
    expect(titles(service)).toEqual(['c', 'b', 'a']);
    await service.setSessionPinned({ sessionId: c.id, pinned: false });
    expect(titles(service)).toEqual(['b', 'a', 'c']);
    expect(live(service).find(s => s.id === c.id)!.pinnedAt).toBeNull();
    expect(snapshotSchema.safeParse(value(service.getSnapshot())).success).toBe(true);
    void a;
  });
  it('remembers the pin through archive and restore, without effect while archived', async () => {
    const repository = new MemoryRepository(), { service } = await start(repository);
    await create(service, 'a'); const b = await create(service, 'b');
    await service.setSessionPinned({ sessionId: b.id, pinned: true });
    const archived = value(await service.settleSession({ sessionId: b.id, confirmActive: true }));
    expect(archived.pinnedAt).toEqual(expect.any(String));
    expect(titles(service)).toEqual(['a']);
    expect(value(service.getHistory({ search: '', status: 'all', page: 1, pageSize: 20 })).items[0]!.pinnedAt).toEqual(archived.pinnedAt);
    const refused = await service.setSessionPinned({ sessionId: b.id, pinned: false });
    expect(refused.ok).toBe(false);
    if (!refused.ok) expect(refused.error.code).toBe('UNSUPPORTED');
    value(await service.unsettleSession({ sessionId: b.id }));
    expect(titles(service)).toEqual(['b', 'a']);
  });
  it('validates pin requests', async () => {
    const { service } = await start(new MemoryRepository());
    expect((await service.setSessionPinned({ sessionId: 'nope', pinned: true })).ok).toBe(false);
    expect((await service.setSessionPinned({ sessionId: randomUUID(), pinned: true } as never)).ok).toBe(false);
  });
});

describe('restart: sessions return without shells; activation opens one fresh terminal', () => {
  it('lists saved sessions with closed tabs, spawns nothing until activation, then applies env to every new terminal', async () => {
    const repository = new MemoryRepository();
    const first = await start(repository);
    const s = await create(first.service, 'project');
    value(await first.service.setSessionEnv({ sessionId: s.id, env: [{ name: 'SHELLFOX_DEMO', value: 'hello' }] }));
    await first.service.setSessionPinned({ sessionId: s.id, pinned: true });
    await first.service.dispose(); // quitting the app closes every owned terminal

    const second = await start(repository);
    const restored = live(second.service);
    expect(restored.map(x => x.title)).toEqual(['project']);
    expect(restored[0]!.pinnedAt).toEqual(expect.any(String));
    expect(restored[0]!.env).toEqual([{ name: 'SHELLFOX_DEMO', value: 'hello' }]);
    expect(restored[0]!.tabs.every(t => t.lifecycle === 'closed')).toBe(true);
    expect(second.processes).toHaveLength(0); // startup opens no terminals

    const activated: SessionDto = value(await second.service.activateSession({ sessionId: s.id }));
    expect(activated.tabs.filter(t => t.lifecycle !== 'closed')).toHaveLength(1);
    expect(second.processes).toHaveLength(1);
    expect(spawnedEnv(second, 0).SHELLFOX_DEMO).toBe('hello');
    // Activating again (double click) does not open a second shell.
    value(await second.service.activateSession({ sessionId: s.id }));
    expect(second.processes).toHaveLength(1);
    // Additional tabs get the session env too.
    value(await second.service.addTab({ sessionId: s.id }));
    expect(second.processes).toHaveLength(2);
    expect(spawnedEnv(second, 1).SHELLFOX_DEMO).toBe('hello');
    await second.service.dispose();
  });
});
