import { it, expect } from 'vitest';
import { randomUUID } from 'node:crypto';
import type { Result, TerminalEvent } from '../../shared/contracts';
import { PtyBackend } from './backend';
import { TerminalDelivery } from './delivery';
import { factoryFixture, launchInput } from './test-fixtures';
const value = <T>(r: Result<T>): T => { if (!r.ok) throw new Error(r.error.code); return r.value; };
async function fixture() {
  const f = factoryFixture(), backend = new PtyBackend(f.options); value(await backend.initialize()); const input = launchInput(); value(await backend.launch(input));
  const received: TerminalEvent[] = [], delivery = new TerminalDelivery(args => backend.attach(args), e => received.push(e));
  backend.subscribe(e => delivery.event(e)); return { ...f, backend, input, received, delivery };
}
it('reattachment cannot reset outstanding live or replay credit without acknowledgement', async () => {
  const f = await fixture(); value(f.delivery.attached(value(f.backend.attach({ tabId: f.input.tabId })))); f.processes[0].output('x'.repeat(128 * 1024));
  for (let i = 0; i < 10; i++) expect(f.delivery.attached(value(f.backend.attach({ tabId: f.input.tabId })))).toMatchObject({ ok: false, error: { code: 'RETRY_CONFIRM_REQUIRED' } });
  expect(f.received.filter(e => e.type === 'data')).toHaveLength(32);
  value(f.delivery.acknowledge({ tabId: f.input.tabId, generation: f.input.generation, sequence: 32 })); value(f.delivery.attached(value(f.backend.attach({ tabId: f.input.tabId }))));
  f.processes[0].output('pending until replay consumed'); expect(f.received.filter(e => e.type === 'data')).toHaveLength(32);
  value(f.delivery.acknowledge({ tabId: f.input.tabId, generation: f.input.generation, sequence: 32 })); expect(f.received.at(-1)).toMatchObject({ type: 'data', data: 'pending until replay consumed' }); await f.backend.dispose();
});
it('forwards activity globally but data only to attached views; detach never kills a terminal', async () => {
  const f = await fixture(); f.processes[0].output('not attached'); expect(f.received).toEqual([{ type: 'activity', tabId: f.input.tabId, busy: true }]);
  const replay = value(f.backend.attach({ tabId: f.input.tabId })); value(f.delivery.attached(replay)); value(f.delivery.acknowledge({ tabId: f.input.tabId, generation: f.input.generation, sequence: replay.lastSequence })); f.processes[0].output('live'); expect(f.received.filter(e => e.type === 'data')).toHaveLength(1);
  value(f.delivery.detach({ tabId: f.input.tabId, generation: f.input.generation })); f.processes[0].output('hidden'); expect(f.received.filter(e => e.type === 'data')).toHaveLength(1); expect(f.processes[0].kill).not.toHaveBeenCalled(); await f.backend.dispose();
});
it('bounds outstanding live output at 128 KiB until acknowledgements', async () => {
  const f = await fixture(); f.delivery.attached(value(f.backend.attach({ tabId: f.input.tabId }))); f.processes[0].output('x'.repeat(200 * 1024));
  const bytes = f.received.filter((e): e is Extract<TerminalEvent, { type: 'data' }> => e.type === 'data').reduce((n, e) => n + Buffer.byteLength(e.data), 0); expect(bytes).toBe(128 * 1024);
  const last = f.received.at(-1)!; expect(last.type).toBe('data'); if (last.type === 'data') value(f.delivery.acknowledge({ tabId: f.input.tabId, generation: f.input.generation, sequence: last.sequence }));
  expect(f.received.filter(e => e.type === 'data').length).toBe(50); await f.backend.dispose();
});
it('reports one replay overflow instead of building an unbounded renderer queue', async () => {
  const f = await fixture(); f.delivery.attached(value(f.backend.attach({ tabId: f.input.tabId }))); f.processes[0].output('x'.repeat(1024 * 1024)); f.processes[0].output('y'.repeat(1024 * 1024));
  expect(f.received.filter(e => e.type === 'error')).toHaveLength(1); expect(f.received.filter(e => e.type === 'data')).toHaveLength(32);
  value(f.delivery.detach({ tabId: f.input.tabId, generation: f.input.generation }));
  const replay = value(f.backend.attach({ tabId: f.input.tabId })); value(f.delivery.attached(replay)); value(f.delivery.acknowledge({ tabId: f.input.tabId, generation: f.input.generation, sequence: replay.lastSequence }));
  f.processes[0].output('recovered'); expect(f.received.at(-1)).toMatchObject({ type: 'data', data: 'recovered' }); await f.backend.dispose();
});
it('validates acknowledgements and stale view generations', async () => {
  const f = await fixture(); f.delivery.attached(value(f.backend.attach({ tabId: f.input.tabId })));
  expect(f.delivery.acknowledge({ tabId: f.input.tabId, generation: randomUUID(), sequence: 0 })).toMatchObject({ ok: false, error: { code: 'TARGET_LOST' } });
  expect(f.delivery.acknowledge({ tabId: f.input.tabId, generation: f.input.generation, sequence: 1 })).toMatchObject({ ok: false, error: { code: 'VALIDATION' } });
  expect(f.delivery.detach({ tabId: f.input.tabId, generation: randomUUID() }).ok).toBe(false); await f.backend.dispose();
});
it('orders exit after the last buffered output and bounds tiny outstanding chunks', async () => {
  const f = await fixture(); f.delivery.attached(value(f.backend.attach({ tabId: f.input.tabId })));
  for (let i = 0; i < 300; i++) f.processes[0].output('x'); f.processes[0].exit(3);
  expect(f.received.filter(e => e.type === 'data')).toHaveLength(256);
  expect(f.received.filter(e => e.type === 'activity')).toEqual([{ type: 'activity', tabId: f.input.tabId, busy: true }, { type: 'activity', tabId: f.input.tabId, busy: false }]); value(f.delivery.acknowledge({ tabId: f.input.tabId, generation: f.input.generation, sequence: 256 }));
  expect(f.received.at(-1)).toMatchObject({ type: 'exit', exitCode: 3, lastSequence: 300 }); await f.backend.dispose();
});
