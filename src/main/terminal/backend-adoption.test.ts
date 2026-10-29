import { randomUUID } from 'node:crypto';
import { spawn } from 'node:child_process';
import { StringDecoder } from 'node:string_decoder';
import { expect, it, vi } from 'vitest';
import type { Result, TerminalBackendEvent } from '../../shared/contracts';
import { terminalSnapshotSchema } from '../../shared/schemas';
import { PtyBackend, type PtyProcess, type TerminalAdoption } from './backend';
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

it('renders a real pipe child without altering the captured stdout and stderr bytes', async () => {
  const stdout = Buffer.from([0, 65, 10, 0xf0, 0x9f, 0x99, 0x82]), stderr = Buffer.from([66, 13, 10, 0xff]);
  const child = spawn(process.execPath, ['-e', `process.stdout.write(Buffer.from(${JSON.stringify([...stdout])})); process.stderr.write(Buffer.from(${JSON.stringify([...stderr])}));`], { stdio: ['pipe', 'pipe', 'pipe'], windowsHide: true });
  if (!child.pid) throw new Error('Child did not start');
  const captured = { stdout: [] as Buffer[], stderr: [] as Buffer[] };
  const listeners = new Set<(data: string) => void>(), exits = new Set<(event: { exitCode: number }) => void>();
  for (const stream of ['stdout', 'stderr'] as const) {
    const decoder = new StringDecoder('utf8');
    child[stream].on('data', (data: Buffer) => { captured[stream].push(data); for (const listener of listeners) listener(decoder.write(data)); });
    child[stream].on('end', () => { const tail = decoder.end(); if (tail) for (const listener of listeners) listener(tail); });
  }
  const closed = new Promise<void>((resolve, reject) => {
    child.once('error', reject);
    child.once('close', code => { for (const listener of [...exits]) listener({ exitCode: code ?? 1 }); resolve(); });
  });
  const adapter: PtyProcess = { pid: child.pid, write: () => { throw new Error('read-only'); }, resize: () => {}, kill: () => { child.kill(); },
    onData: listener => { listeners.add(listener); return { dispose: () => { listeners.delete(listener); } }; },
    onExit: listener => { exits.add(listener); return { dispose: () => { exits.delete(listener); } }; },
  };
  const backend = new PtyBackend(), input = identity();
  try {
    value(backend.adopt(input, adapter));
    await closed;
    expect(Buffer.concat(captured.stdout)).toEqual(stdout); expect(Buffer.concat(captured.stderr)).toEqual(stderr);
    const replay = value(backend.attach({ tabId: input.tabId }));
    const text = replay.chunks.map(chunk => chunk.data).join('');
    expect(text).toContain(stdout.toString('utf8')); expect(text).toContain(stderr.toString('utf8'));
    expect(replay).toMatchObject({ state: 'closed', exitCode: 0 });
  } finally { child.kill(); await closed; await backend.dispose(); }
});
