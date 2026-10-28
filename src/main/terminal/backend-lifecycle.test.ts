import { expect, it, vi } from 'vitest';
import type { Result } from '../../shared/contracts';
import { PtyBackend } from './backend';
import { TerminalDelivery } from './delivery';
import { factoryFixture, launchInput } from './test-fixtures';
import { terminalSnapshotSchema } from '../../shared/schemas';
import { tuiStream } from './tui-fixture';

const value = <T>(result: Result<T>): T => {
  if (!result.ok) throw new Error(result.error.code);
  return result.value;
};

// Inspect only ownership references, so a regression does not dump xterm's cyclic graph.
function field(object: unknown, key: string): unknown {
  return typeof object === 'object' && object !== null ? Reflect.get(object, key) : undefined;
}
function resources(backend: PtyBackend, tabId: string) {
  const entries = field(backend, 'entries');
  const entry: unknown = entries instanceof Map ? entries.get(tabId) : undefined;
  const mirror = field(entry, 'mirror');
  return {
    ptyReleased: field(entry, 'pty') === null,
    handles: field(entry, 'handles'),
    parserReleased: ['term', 'parseNow', 'serializer'].every(key => field(mirror, key) === null),
  };
}

it.each(['win32', 'linux', 'darwin'] as const)('releases closed PTY resources after confirmed cleanup on %s, keeping replay and exit delivery', async platform => {
  const f = factoryFixture();
  const status = { version: 1 as const, ok: true, state: 'open' as const, supervisorPid: 123, shellPid: 123, shellBirth: 'darwin:123:456', shellAlive: true, exitCode: null, reason: null };
  const backend = new PtyBackend({ ...f.options, platform, prepareSupervisor: async (_profile, cwd) => ({ file: '/fake/supervisor', args: [], cwd, env: {}, control: {
    ready: async () => status,
    close: async () => { f.processes[0].exit(0); return { ...status, state: 'closed', shellAlive: false, exitCode: 0 }; },
    disposeConfirmed: async () => {},
  } }) });
  value(await backend.initialize());
  const input = launchInput();
  value(await backend.launch(input));
  const pty = f.processes[0];
  const received: string[] = [];
  const delivery = new TerminalDelivery(args => backend.attach(args), event => received.push(event.type));
  backend.subscribe(event => delivery.event(event));
  value(delivery.attached(value(backend.attach({ tabId: input.tabId }))));
  for (let i = 0; i < 300; i++) pty.output(`line ${i}\r\n`);
  value(await backend.close({ tabId: input.tabId, generation: input.generation }));
  expect(backend.live()).toHaveLength(0);
  expect(resources(backend, input.tabId)).toEqual({ ptyReleased: true, handles: [], parserReleased: true });
  expect(received.at(-1)).not.toBe('exit');
  value(delivery.acknowledge({ tabId: input.tabId, generation: input.generation, sequence: 256 }));
  expect(received.at(-1)).toBe('exit');
  const replay = value(backend.attach({ tabId: input.tabId }));
  expect(replay).toMatchObject({ state: 'closed', exitCode: 0, lastSequence: 300 });
  expect(replay.chunks.map(chunk => chunk.data).join('')).toContain('line 299');
  expect(pty).toMatchObject({ data: new Set(), exits: new Set() });
  delivery.dispose();
  await backend.dispose();
});

it('releases resources on natural exit while retaining an identical restorable TUI snapshot', async () => {
  const f = factoryFixture(), backend = new PtyBackend(f.options);
  value(await backend.initialize());
  const input = launchInput();
  value(await backend.launch(input));
  const pty = f.processes[0];
  for (const chunk of tuiStream({ cols: 80, rows: 24, frames: 4000 })) pty.output(chunk);
  const before = value(backend.attach({ tabId: input.tabId }));
  expect(before.snapshot).toBe(true);
  pty.exit(9);
  expect(resources(backend, input.tabId)).toEqual({ ptyReleased: true, handles: [], parserReleased: true });
  expect(pty).toMatchObject({ data: new Set(), exits: new Set() });
  const after = value(backend.attach({ tabId: input.tabId }));
  expect(terminalSnapshotSchema.safeParse(after).success).toBe(true);
  expect(after).toMatchObject({ state: 'closed', exitCode: 9, snapshot: true });
  expect(after.chunks).toEqual(before.chunks);
  await backend.dispose();
});

it('retains a running PTY and parser after an unconfirmed close timeout', async () => {
  const f = factoryFixture(), backend = new PtyBackend({ ...f.options, closeTimeoutMs: 1 });
  value(await backend.initialize());
  const input = launchInput();
  value(await backend.launch(input));
  f.processes[0].kill.mockImplementation(() => {});
  expect((await backend.close({ tabId: input.tabId, generation: input.generation })).ok).toBe(false);
  expect(resources(backend, input.tabId)).toMatchObject({ ptyReleased: false, parserReleased: false });
  expect(backend.live()).toHaveLength(1);
  f.processes[0].exit(0);
  expect(resources(backend, input.tabId)).toEqual({ ptyReleased: true, handles: [], parserReleased: true });
  await backend.dispose();
});

it('keeps cleanup ownership after natural exit until descendant cleanup succeeds', async () => {
  const f = factoryFixture();
  const terminateUnix = vi.fn<() => Promise<void>>(async () => { throw new Error('unconfirmed descendants'); });
  const backend = new PtyBackend({ ...f.options, platform: 'linux', terminateUnix });
  value(await backend.initialize());
  const input = launchInput();
  value(await backend.launch(input));
  const pty = f.processes[0];
  pty.output('final screen');
  pty.exit(7);
  await backend.awaitCleanup(input.sessionId);
  expect(backend.live()).toHaveLength(1);
  expect(resources(backend, input.tabId)).toMatchObject({ ptyReleased: false, parserReleased: true });
  expect(backend.get(input.tabId)?.cleanupPending).toBe(true);
  expect(await backend.launch({ ...launchInput(), tabId: input.tabId })).toMatchObject({ ok: false });
  terminateUnix.mockImplementation(async () => {});
  value(await backend.close({ tabId: input.tabId, generation: input.generation }));
  expect(resources(backend, input.tabId)).toEqual({ ptyReleased: true, handles: [], parserReleased: true });
  expect(backend.get(input.tabId)?.cleanupPending).toBe(false);
  expect(value(backend.attach({ tabId: input.tabId })).chunks[0].data).toBe('final screen');
  await backend.dispose();
});
