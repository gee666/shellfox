import { randomUUID } from 'node:crypto';
import { afterEach, expect, it, vi } from 'vitest';
import { success, type Result, type TerminalAttachmentDto, type TerminalEvent } from '../../shared/contracts';
import { responseSchemas } from '../../shared/schemas';
import { managerOperation } from '../../host/operations';
import { remoteOperation } from '../remote/operations';
import { embeddedFixture } from '../remote/test-fixtures';

const cleanups: Array<() => Promise<void>> = [];
afterEach(async () => { for (const cleanup of cleanups.splice(0).reverse()) await cleanup(); vi.restoreAllMocks(); });
async function fixture() {
  const f = await embeddedFixture(); cleanups.push(() => f.service.dispose());
  const creatorEvents: TerminalEvent[] = [], otherEvents: TerminalEvent[] = [];
  const creator = f.service.createTerminalView(event => creatorEvents.push(event)), other = f.service.createTerminalView(event => otherEvents.push(event));
  return { ...f, creator, other, creatorEvents, otherEvents };
}
const value = <T>(result: Result<T>): T => { if (!result.ok) throw new Error(result.error.message); return result.value; };

for (const transport of ['embedded', 'rpc', 'remote'] as const) {
  for (const method of ['createSession', 'addTab', 'activateSession'] as const) {
    it(`${transport} reserves ${method}'s new terminal before eager viewers attach during launch`, async () => {
      const f = await fixture();
      const saved = method === 'createSession' ? undefined : value(await f.service.createSession({ cwd: '/work', requestId: randomUUID() }));
      if (saved && method === 'activateSession') f.processes[0].exit(0);
      const payload = saved ? { sessionId: saved.id } : { cwd: '/work', requestId: randomUUID() };
      const attempts: Result<TerminalAttachmentDto>[] = [];
      const launch = f.backend.launch.bind(f.backend);
      vi.spyOn(f.backend, 'launch').mockImplementation(async input => {
        // Reservation exists even before the backend can return an identity.
        expect(f.creator.tabIds.has(input.tabId)).toBe(true);
        const result = await launch(input);
        attempts.push(f.other.attach({ tabId: input.tabId, generation: input.generation }));
        f.processes.at(-1)!.output('before creator attaches');
        return result;
      });
      const result = transport === 'rpc'
        ? await managerOperation({ service: f.service, delivery: f.creator, clientId: 'creator' }, method, payload)
        : transport === 'remote' ? await remoteOperation(f.service, f.creator, 'creator', method, payload)
          : method === 'createSession' ? await f.service.createSession({ cwd: '/work', requestId: randomUUID() }, f.creator)
            : method === 'addTab' ? await f.service.addTab({ sessionId: saved!.id }, f.creator)
              : await f.service.activateSession({ sessionId: saved!.id }, f.creator);
      const session = value(responseSchemas[method].parse(result)), tab = session.tabs.at(-1)!;
      const identity = { tabId: tab.id, generation: tab.generation! };
      expect(attempts).toEqual([success(expect.objectContaining({ control: 'blocked', ...identity }))]);
      expect([...f.creatorEvents, ...f.otherEvents].some(event => event.type === 'data')).toBe(false);
      const owned = value(f.creator.attach(identity));
      if (owned.control !== 'owned') throw new Error('Creator was blocked');
      expect(owned.chunks.map(chunk => chunk.data).join('')).toContain('before creator attaches');
      expect(f.other.attach(identity)).toMatchObject({ ok: true, value: { control: 'blocked' } });
      // Creation priority is not a permanent lock against an explicit Take it.
      expect(f.other.take(identity)).toEqual(success({ taken: true }));
      expect(f.other.attach(identity)).toMatchObject({ ok: true, value: { control: 'owned' } });
      expect(f.creator.attach(identity)).toMatchObject({ ok: true, value: { control: 'blocked' } });
    });
  }
}

it('keeps the first creator reservation for concurrent duplicate requests from different views', async () => {
  const f = await fixture(), input = { cwd: '/work', requestId: randomUUID() };
  const [first, duplicate] = await Promise.all([f.service.createSession(input, f.creator), f.service.createSession(input, f.other)]);
  const session = value(first);
  expect(value(duplicate).id).toBe(session.id); expect(f.processes).toHaveLength(1);
  expect(f.other.attach({ tabId: session.tabs[0].id })).toMatchObject({ ok: true, value: { control: 'blocked' } });
  expect(f.creator.attach({ tabId: session.tabs[0].id })).toMatchObject({ ok: true, value: { control: 'owned' } });
});

it('does not reclaim an existing terminal on a duplicate create or ordinary activation', async () => {
  const f = await fixture(), input = { cwd: '/work', requestId: randomUUID() };
  const session = value(await f.service.createSession(input, f.creator)), tab = session.tabs[0];
  const identity = { tabId: tab.id, generation: tab.generation! };
  f.other.attach(identity); f.other.take(identity);
  const owner = value(f.other.attach(identity));
  expect(value(await f.service.createSession(input, f.creator)).id).toBe(session.id);
  await f.service.activateSession({ sessionId: session.id }, f.creator);
  expect(f.creator.attach(identity)).toMatchObject({ ok: true, value: { control: 'blocked', controlEpoch: owner.controlEpoch } });
  expect(f.processes).toHaveLength(1);
});

it('releases unconsumed creation reservations on creator disconnect and promotes a waiting viewer', async () => {
  const f = await fixture(), session = value(await f.service.createSession({ cwd: '/work', requestId: randomUUID() }, f.creator));
  const identity = { tabId: session.tabs[0].id, generation: session.tabs[0].generation! };
  expect(f.other.attach(identity)).toMatchObject({ ok: true, value: { control: 'blocked' } });
  f.creator.dispose();
  expect(f.other.attach(identity)).toMatchObject({ ok: true, value: { control: 'owned' } });
  expect(f.processes[0].kill).not.toHaveBeenCalled();
});

it.each(['before reservation', 'during startup'] as const)('does not retain a disconnected creator %s', async timing => {
  const f = await fixture(), launch = f.backend.launch.bind(f.backend);
  if (timing === 'before reservation') f.creator.dispose();
  vi.spyOn(f.backend, 'launch').mockImplementation(async input => {
    const pending = launch(input);
    if (timing === 'during startup') f.creator.dispose();
    const result = await pending;
    expect(f.other.attach({ tabId: input.tabId })).toMatchObject({ ok: true, value: { control: 'owned' } });
    return result;
  });
  expect(await f.service.createSession({ cwd: '/work', requestId: randomUUID() }, f.creator)).toMatchObject({ ok: true });
  expect(f.creator.tabIds.size).toBe(0);
});

it('cleans reservations after failed launch, immediate exit and persistence failure', async () => {
  const f = await fixture();
  const launch = vi.spyOn(f.backend, 'launch').mockResolvedValueOnce({ ok: false, error: { code: 'LAUNCH_FAILED', message: 'Failed', retryable: false } });
  expect(await f.service.createSession({ cwd: '/work', requestId: randomUUID() }, f.creator)).toMatchObject({ ok: false });
  expect(f.creator.tabIds.size).toBe(0);
  launch.mockRestore();
  const original = f.backend.launch.bind(f.backend);
  vi.spyOn(f.backend, 'launch').mockImplementationOnce(async input => { const result = await original(input); f.processes.at(-1)!.exit(0); return result; });
  expect(await f.service.createSession({ cwd: '/work', requestId: randomUUID() }, f.creator)).toMatchObject({ ok: false });
  expect(f.creator.tabIds.size).toBe(0);
  vi.spyOn(f.repository, 'saveTab').mockImplementationOnce(() => { throw new Error('Storage failed'); });
  expect(await f.service.createSession({ cwd: '/work', requestId: randomUUID() }, f.creator)).toMatchObject({ ok: false, error: { code: 'STORAGE_FAILED' } });
  expect(f.creator.tabIds.size).toBe(0);
});

it('enforces the per-view limit before persisting or launching another terminal', async () => {
  const f = await fixture();
  for (let index = 0; index < 16; index++) value(await f.service.createSession({ cwd: '/work', requestId: randomUUID() }, f.creator));
  expect(await f.service.createSession({ cwd: '/work', requestId: randomUUID() }, f.creator)).toMatchObject({ ok: false, error: { code: 'VALIDATION' } });
  expect(f.processes).toHaveLength(16); expect(f.repository.sessions()).toHaveLength(16);
  f.creator.dispose();
  expect(f.creator.tabIds.size).toBe(0);
});
