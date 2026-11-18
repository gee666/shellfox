import { randomUUID } from 'node:crypto';
import { expect, it, vi } from 'vitest';
import type { Result, TerminalBackendEvent } from '../../shared/contracts';
import { terminalSnapshotSchema } from '../../shared/schemas';
import { PtyBackend, type TerminalAdoption } from './backend';
import { FakePty, factoryFixture } from './test-fixtures';

const value = <T>(result: Result<T>): T => { if (!result.ok) throw new Error(result.error.code); return result.value; };
const identity = (): TerminalAdoption => ({ tabId: randomUUID(), sessionId: randomUUID(), generation: randomUUID(), cwd: '/work' });

it.each(['win32', 'linux', 'darwin'] as const)('adopts a read-only display without shell startup and delegates cleanup on %s', async platform => {
  const f = factoryFixture(), pty = new FakePty(), terminateUnix = vi.fn(), terminateGuest = vi.fn();
  const backend = new PtyBackend({ ...f.options, platform, terminateUnix, terminateGuest });
  const input = identity(), events: TerminalBackendEvent[] = [];
  backend.subscribe(event => events.push(event));
  expect(value(backend.adopt(input, pty))).toMatchObject({ viewOnly: true, profileId: 'automation-pipe', root: { pid: pty.pid }, cleanupPending: false });
  expect(f.factory).not.toHaveBeenCalled();
  pty.output('stdout\n'); pty.output('stderr\r\n\x1b[31mred\x1b[0m\u0000🙂');
  const replay = value(backend.attach({ tabId: input.tabId }));
  expect(replay.chunks.map(chunk => chunk.data).join('')).toBe('stdout\nstderr\r\n\x1b[31mred\x1b[0m\u0000🙂');
  expect(terminalSnapshotSchema.safeParse(replay).success).toBe(true);
  for (const userInitiated of [true, false]) expect(backend.write({ tabId: input.tabId, generation: input.generation, data: 'not stdin', userInitiated })).toMatchObject({ ok: false, error: { code: 'UNSUPPORTED' } });
  // Parser-generated replies must not reach automation stdin either.
  pty.output('\x1b[6n\x1b[c');
  value(backend.attach({ tabId: input.tabId }));
  expect(pty.write).not.toHaveBeenCalled();
  value(backend.resize({ tabId: input.tabId, generation: input.generation, cols: 100, rows: 40 }));
  value(await backend.close({ tabId: input.tabId, generation: input.generation }));
  expect(pty.kill).toHaveBeenCalledTimes(1);
  expect(terminateUnix).not.toHaveBeenCalled(); expect(terminateGuest).not.toHaveBeenCalled();
  expect(events.filter(event => event.type === 'exit')).toHaveLength(1);
  expect(value(backend.attach({ tabId: input.tabId }))).toMatchObject({ state: 'closed', exitCode: 0 });
  expect(pty).toMatchObject({ data: new Set(), exits: new Set() });
  await backend.dispose();
});

it('waits for adapter exit evidence and retains failed cleanup for retry', async () => {
  const pty = new FakePty(), backend = new PtyBackend({ platform: 'linux', closeTimeoutMs: 1 }), input = identity();
  value(backend.adopt(input, pty)); pty.kill.mockImplementation(() => {});
  expect(await backend.close({ tabId: input.tabId, generation: input.generation })).toMatchObject({ ok: false });
  expect(backend.live()).toHaveLength(1);
  expect(backend.adopt(input, new FakePty())).toMatchObject({ ok: false });
  pty.kill.mockImplementation(() => pty.exit(5));
  value(await backend.close({ tabId: input.tabId, generation: input.generation }));
  expect(value(backend.attach({ tabId: input.tabId }))).toMatchObject({ state: 'closed', exitCode: 5 });
  await backend.dispose();
});

it('releases subscriptions even if an already-exited adapter calls listeners synchronously', async () => {
  const pty = new FakePty(), backend = new PtyBackend(), input = identity(), dataDisposed = vi.fn(), exitDisposed = vi.fn();
  vi.spyOn(pty, 'onData').mockImplementation(listener => { listener('finished'); return { dispose: dataDisposed }; });
  vi.spyOn(pty, 'onExit').mockImplementation(listener => { listener({ exitCode: 3 }); return { dispose: exitDisposed }; });
  expect(value(backend.adopt(input, pty))).toMatchObject({ state: 'closed', exitCode: 3 });
  expect(dataDisposed).toHaveBeenCalledTimes(1); expect(exitDisposed).toHaveBeenCalledTimes(1);
  expect(value(backend.attach({ tabId: input.tabId })).chunks[0].data).toBe('finished');
  expect(pty.kill).not.toHaveBeenCalled();
  await backend.dispose();
});
