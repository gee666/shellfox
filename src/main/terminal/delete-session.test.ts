import { describe, it, expect } from 'vitest';
import { randomUUID } from 'node:crypto';
import type { Result } from '../../shared/contracts';
import { PtyBackend } from './backend';
import { EmbeddedSessionService } from './service';
import { MemoryRepository, factoryFixture } from './test-fixtures';

const value = <T>(r: Result<T>): T => { if (!r.ok) throw new Error(r.error.code + ': ' + r.error.message); return r.value; };
async function start(repository = new MemoryRepository()) {
  const f = factoryFixture();
  const service = new EmbeddedSessionService(repository, new PtyBackend(f.options), () => ({ setWatch: () => {}, dispose: () => {} }));
  await service.initialize();
  return { ...f, repository, service };
}
const create = async (service: EmbeddedSessionService, title: string) => value(await service.createSession({ cwd: '/work/' + title, title, requestId: randomUUID() }));
const sessions = (service: EmbeddedSessionService) => value(service.getSnapshot()).sessions;

describe('permanent session deletion', () => {
  it('deletes an unsettled live session after closing its owned terminal', async () => {
    const { service, repository, processes } = await start(); const a = await create(service, 'a');
    expect(value(await service.deleteSession({ sessionId: a.id }))).toEqual({ deleted: true });
    expect(repository.session(a.id)).toBeUndefined(); expect(processes[0].kill).toHaveBeenCalledTimes(1);
    await service.dispose();
  });
  it('closes a settled session\'s live terminals before deleting it', async () => {
    const { service, repository, processes } = await start(); const a = await create(service, 'a'), b = await create(service, 'b');
    value(await service.settleSession({ sessionId: a.id, confirmActive: true }));
    expect(value(await service.deleteSession({ sessionId: a.id }))).toEqual({ deleted: true });
    expect(processes[0]!.kill).toHaveBeenCalledTimes(1); expect(processes[1]!.kill).not.toHaveBeenCalled();
    expect(repository.session(a.id)).toBeUndefined(); expect(repository.tabs(a.id)).toEqual([]);
    expect(repository.session(b.id)).toBeDefined();
    await service.dispose();
  });
  it('keeps the session when a live terminal cannot be confirmed closed', async () => {
    const { service, repository, processes } = await start(); const a = await create(service, 'a');
    repository.saveSession({ ...repository.session(a.id)!, settledAt: new Date().toISOString() });
    processes[0]!.kill.mockImplementation(() => { throw new Error('denied'); });
    expect(await service.deleteSession({ sessionId: a.id })).toMatchObject({ ok: false });
    expect(repository.session(a.id)).toBeDefined(); expect(repository.tabs(a.id)).toHaveLength(1);
    processes[0]!.kill.mockReset(); processes[0]!.exit(0);
    expect(value(await service.deleteSession({ sessionId: a.id }))).toEqual({ deleted: true });
    await service.dispose();
  });
  it('deletes only the requested settled session with its tabs and operations', async () => {
    const { service, repository, processes } = await start();
    const a = await create(service, 'a'), b = await create(service, 'b'), c = await create(service, 'c');
    for (const id of [a.id, b.id]) value(await service.settleSession({ sessionId: id, confirmActive: true }));
    processes[0]!.exit(0); processes[1]!.exit(0);
    const events: string[] = []; service.subscribe(event => events.push(event.reason));
    expect(value(await service.deleteSession({ sessionId: a.id }))).toEqual({ deleted: true });
    expect(events).toEqual(['sessions']);
    expect(repository.session(a.id)).toBeUndefined(); expect(repository.tabs(a.id)).toEqual([]);
    expect([...repository.operationMap.values()].filter(o => o.sessionId === a.id)).toEqual([]);
    expect(repository.tabs(b.id)).toEqual([]); expect(repository.session(c.id)).toBeDefined();
    expect(sessions(service).map(item => item.id).sort()).toEqual([b.id, c.id].sort());
    expect(await service.deleteSession({ sessionId: a.id })).toMatchObject({ ok: false, error: { code: 'NOT_FOUND' } });
    await service.dispose();
  });
  it('validates the request', async () => {
    const { service } = await start();
    expect(await service.deleteSession({ sessionId: 'nope' })).toMatchObject({ ok: false, error: { code: 'VALIDATION' } });
    expect(await service.deleteSession({ sessionId: randomUUID(), extra: true } as never)).toMatchObject({ ok: false, error: { code: 'VALIDATION' } });
    await service.dispose();
  });
});
