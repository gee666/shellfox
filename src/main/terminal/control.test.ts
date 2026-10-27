import { afterEach, expect, it } from 'vitest';
import type { TerminalEvent, TerminalAttachmentDto, OwnedTerminalAttachmentDto } from '../../shared/contracts';
import { embeddedFixture } from '../remote/test-fixtures';
import { randomUUID } from 'node:crypto';
import { terminalAttachmentSchema } from '../../shared/schemas';
const cleanups: Array<() => Promise<void>> = [];
afterEach(async () => { for (const cleanup of cleanups.splice(0)) await cleanup(); });
async function fixture() {
  const f = await embeddedFixture(); cleanups.push(() => f.service.dispose());
  const created = await f.service.createSession({ cwd: '/work', requestId: randomUUID() });
  if (!created.ok) throw new Error(created.error.message);
  const tab = created.value.tabs[0], identity = { tabId: tab.id, generation: tab.generation! };
  function client() {
    let events: TerminalEvent[] = [];
    const view = f.service.createTerminalView(event => { events.push(event); }, tabId => { events = events.filter(event => event.tabId !== tabId); });
    return { view, events: () => events };
  }
  return { ...f, identity, client };
}
function owned(value: TerminalAttachmentDto): OwnedTerminalAttachmentDto {
  if (value.control !== 'owned') throw new Error('Expected owner'); return value;
}
function attach(client: ReturnType<Awaited<ReturnType<typeof fixture>>['client']>, identity: { tabId: string; generation: string }) {
  const result = client.view.attach(identity); if (!result.ok) throw new Error(result.error.message); return result.value;
}
it('gives only one client replay/output/input/resize/ACK and returns no contents to blocked clients', async () => {
  const f = await fixture(), a = f.client(), b = f.client(); f.processes[0].output('private screen');
  const first = owned(attach(a, f.identity)), blocked = attach(b, f.identity);
  expect(first.chunks.map(chunk => chunk.data).join('')).toContain('private screen');
  expect(blocked).toEqual({ ...f.identity, sessionId: first.sessionId, control: 'blocked', controlEpoch: first.controlEpoch });
  expect(terminalAttachmentSchema.safeParse(blocked).success).toBe(true);
  expect(Object.keys(blocked).sort()).toEqual(['control', 'controlEpoch', 'generation', 'sessionId', 'tabId']);
  f.processes[0].output('more private screen');
  expect(b.events().filter(event => event.type === 'data')).toEqual([]);
  const stale = { ...f.identity, controlEpoch: first.controlEpoch };
  expect(b.view.write({ ...stale, data: 'forbidden' })).toMatchObject({ ok: false, error: { code: 'TERMINAL_CONTROL_REQUIRED' } });
  expect(b.view.resize({ ...stale, cols: 120, rows: 30 })).toMatchObject({ ok: false, error: { code: 'TERMINAL_CONTROL_REQUIRED' } });
  expect(b.view.acknowledge({ ...stale, sequence: first.lastSequence })).toMatchObject({ ok: false, error: { code: 'TERMINAL_CONTROL_REQUIRED' } });
  expect(f.processes[0].write).not.toHaveBeenCalled(); expect(f.processes[0].resize).not.toHaveBeenCalled();
});
it('takes atomically, purges queued unacknowledged frames and requires fresh attach before new-owner I/O', async () => {
  const f = await fixture(), a = f.client(), b = f.client();
  const first = owned(attach(a, f.identity)); attach(b, f.identity);
  f.processes[0].output('queued before take'); expect(a.events().some(event => event.type === 'data')).toBe(true);
  expect(b.view.take(f.identity)).toEqual({ ok: true, value: { taken: true } });
  expect(a.events().every(event => event.type !== 'data')).toBe(true);
  expect(a.events()).toContainEqual(expect.objectContaining({ type: 'control', control: 'blocked' }));
  const grant = b.events().find(event => event.type === 'control');
  if (!grant || grant.type !== 'control') throw new Error('No grant');
  expect(Object.keys(grant).sort()).toEqual(['control', 'controlEpoch', 'generation', 'tabId', 'type']);
  f.processes[0].output('before fresh attach'); expect(b.events().some(event => event.type === 'data')).toBe(false);
  expect(b.view.write({ ...f.identity, controlEpoch: grant.controlEpoch, data: 'too early' })).toMatchObject({ ok: false, error: { code: 'TERMINAL_CONTROL_REQUIRED' } });
  const next = owned(attach(b, f.identity));
  expect(next.chunks.map(chunk => chunk.data).join('')).toContain('before fresh attach');
  expect(next.controlEpoch).toBe(grant.controlEpoch);
  expect(a.view.write({ ...f.identity, controlEpoch: first.controlEpoch, data: 'late' })).toMatchObject({ ok: false, error: { code: 'TERMINAL_CONTROL_REQUIRED' } });
  expect(b.view.write({ ...f.identity, controlEpoch: next.controlEpoch, data: 'owned' })).toEqual({ ok: true, value: { written: true } });
  expect(f.processes[0].write).toHaveBeenCalledWith('owned');
});
it('fences stale input/resize/ACK after the same client retakes ownership', async () => {
  const f = await fixture(), a = f.client(), b = f.client();
  const first = owned(attach(a, f.identity)); attach(b, f.identity); b.view.take(f.identity); attach(b, f.identity);
  a.view.take(f.identity); const current = owned(attach(a, f.identity)); expect(current.controlEpoch).toBeGreaterThan(first.controlEpoch);
  const stale = { ...f.identity, controlEpoch: first.controlEpoch };
  expect(a.view.write({ ...stale, data: 'old queued input' })).toMatchObject({ ok: false, error: { code: 'TERMINAL_CONTROL_REQUIRED' } });
  expect(a.view.resize({ ...stale, cols: 80, rows: 25 })).toMatchObject({ ok: false, error: { code: 'TERMINAL_CONTROL_REQUIRED' } });
  expect(a.view.acknowledge({ ...stale, sequence: first.lastSequence })).toMatchObject({ ok: false, error: { code: 'TERMINAL_CONTROL_REQUIRED' } });
});
it('promotes waiting viewers FIFO on detach/disconnect without a timer or implicit Take', async () => {
  const f = await fixture(), a = f.client(), b = f.client(), c = f.client();
  attach(a, f.identity); attach(b, f.identity); attach(c, f.identity); attach(b, f.identity);
  a.view.dispose();
  expect(b.events()).toContainEqual(expect.objectContaining({ type: 'control', control: 'owned' }));
  expect(c.events().some(event => event.type === 'control' && event.control === 'owned')).toBe(false);
  attach(b, f.identity); b.view.detach(f.identity);
  expect(c.events()).toContainEqual(expect.objectContaining({ type: 'control', control: 'owned' }));
});
it('replaces owned delivery credit without releasing ownership or promoting a blocked viewer', async () => {
  const f = await fixture(), a = f.client(), b = f.client();
  const first = owned(attach(a, f.identity)); attach(b, f.identity);
  f.processes[0].output('unacked'); const refreshed = owned(attach(a, f.identity));
  expect(refreshed.controlEpoch).toBeGreaterThan(first.controlEpoch);
  expect(refreshed.chunks.map(chunk => chunk.data).join('')).toContain('unacked');
  expect(b.events().some(event => event.type === 'control' && event.control === 'owned')).toBe(false);
  expect(a.view.acknowledge({ ...f.identity, controlEpoch: first.controlEpoch, sequence: first.lastSequence })).toMatchObject({ ok: false, error: { code: 'TERMINAL_CONTROL_REQUIRED' } });
});
it('notifies blocked waiters of exit without replay and does not promote them onto a dead shell', async () => {
  const f = await fixture(), a = f.client(), b = f.client();
  const owner = owned(attach(a, f.identity)); attach(b, f.identity);
  f.processes[0].output('private before exit'); f.processes[0].exit(7);
  expect(b.events()).toEqual([expect.objectContaining({ type: 'exit', ...f.identity, controlEpoch: owner.controlEpoch, exitCode: 7 })]);
  expect(b.view.tabIds.size).toBe(0);
  a.view.dispose(); expect(b.events().some(event => event.type === 'control' && event.control === 'owned')).toBe(false);
});
it('clears the sixteen-view limit on detach instead of leaking strict-schema slot objects', async () => {
  const f = await fixture(), a = f.client();
  for (let index = 0; index < 20; index++) { owned(attach(a, f.identity)); expect(a.view.detach(f.identity)).toEqual({ ok: true, value: { detached: true } }); }
  expect(a.view.tabIds.size).toBe(0);
});
