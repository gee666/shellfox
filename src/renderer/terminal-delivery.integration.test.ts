// @vitest-environment jsdom
import './terminal-test-mocks';
import { afterEach, describe, expect, it, vi } from 'vitest';
import type { Result, TerminalApi, TerminalEvent } from '../shared/contracts';
import { failure, success } from '../shared/contracts';
import { PtyBackend } from '../main/terminal/backend';
import { TerminalDelivery } from '../main/terminal/delivery';
import { factoryFixture, launchInput } from '../main/terminal/test-fixtures';
import { TerminalRegistry, type SurfaceFactory } from './terminal-client';
import { deferred } from './test-fixtures';

const value = <T>(result: Result<T>): T => { if (!result.ok) throw new Error(result.error.code); return result.value; };
const settle = async () => { for (let i = 0; i < 60; i++) await Promise.resolve(); };
interface Screen { output: string; callbacks: (() => void)[]; disposed: boolean; input: (data: string) => void }
const cleanups: (() => Promise<void>)[] = [];
afterEach(async () => { for (const cleanup of cleanups.splice(0)) await cleanup(); document.body.replaceChildren(); });

async function fixture() {
  // Only the PTY process and asynchronous xterm parser are controlled doubles.
  // Replay eviction, chunking, credit, attach refusal and renderer ACK/recovery
  // use the actual production PtyBackend, TerminalDelivery and TerminalRegistry.
  const pty = factoryFixture(); const backend = new PtyBackend(pty.options);
  value(await backend.initialize()); const input = launchInput(); value(await backend.launch(input));
  const listeners = new Set<(event: TerminalEvent) => void>();
  const delivered: TerminalEvent[] = []; const trace: string[] = [];
  const delivery = new TerminalDelivery(args => backend.attach(args), event => {
    delivered.push(event); for (const listener of listeners) listener(event);
  });
  const off = backend.subscribe(event => delivery.event(event));
  const screens: Screen[] = [];
  const factory: SurfaceFactory = input => {
    const screen: Screen = { output: '', callbacks: [], disposed: false, input }; screens.push(screen);
    return {
      element: document.createElement('div'),
      write(data, done) { screen.output += data; screen.callbacks.push(done); },
      reset() { screen.output = ''; }, fit: () => ({ cols: 80, rows: 24 }), focus() {}, setInput() {},
      dispose() { screen.disposed = true; },
    };
  };
  let ackBarrier: (() => Promise<void>) | undefined;
  const api: TerminalApi = {
    getTerminalProfiles: async () => success(backend.getProfiles()),
    subscribeTerminal(listener) { listeners.add(listener); return () => { listeners.delete(listener); }; },
    attachTerminal: vi.fn(async args => {
      trace.push('attach');
      const replay = backend.attach(args); if (!replay.ok) return replay;
      const registered = delivery.attached(replay.value);
      return registered.ok ? replay : registered;
    }),
    acknowledgeTerminal: vi.fn(async args => {
      trace.push(`ack:${args.sequence}`);
      const result = delivery.acknowledge(args);
      const barrier = ackBarrier; ackBarrier = undefined;
      if (barrier) { await barrier(); trace.push('old-ack-complete'); return failure('INTERNAL', 'Delayed old view ACK failure'); }
      return result;
    }),
    detachTerminal: vi.fn(async args => { trace.push('detach'); return delivery.detach(args); }),
    writeTerminal: vi.fn(async args => backend.write(args)),
    resizeTerminal: async args => backend.resize(args),
    closeTab: vi.fn(async () => failure('UNSUPPORTED', 'Test must not close a shell to recover output')),
  };
  const registry = new TerminalRegistry(api, factory);
  const host = document.createElement('div'); document.body.append(host);
  registry.start();
  cleanups.push(async () => { registry.dispose(); await settle(); off(); delivery.dispose(); await backend.dispose(); });
  const latest = () => screens.at(-1)!;
  async function parseAll() {
    // The real registry batches tiny chunks, rather than requiring one callback
    // per chunk. Flush each asynchronous write and its delivery/ACK microtasks.
    for (let i = 0; i < 10000; i++) {
      await settle(); const callback = latest().callbacks.shift();
      if (!callback) { await settle(); if (!latest().callbacks.length) return; continue; }
      callback();
    }
    throw new Error('Renderer/parser failed to settle');
  }
  return { ...pty, backend, delivery, input, api, registry, host, trace, delivered, screens, latest, parseAll,
    delayNextAck: (barrier: () => Promise<void>) => { ackBarrier = barrier; },
  };
}

describe('TerminalRegistry + production TerminalDelivery credit/replay integration', () => {
  it.each([3000, 8192])('consumes all %i one-byte replay chunks and releases replay credit', async count => {
    const f = await fixture(); for (let i = 0; i < count; i++) f.processes[0]!.output('x');
    const replay = value(f.backend.attach({ tabId: f.input.tabId })); expect(replay.chunks).toHaveLength(count);
    f.registry.mount(f.input.tabId, f.host, true); await settle();
    expect(f.api.acknowledgeTerminal).not.toHaveBeenCalled();
    f.processes[0]!.output('LIVE');
    expect(f.delivered.filter(event => event.type === 'data')).toHaveLength(0);
    await f.parseAll();
    expect(f.latest().output).toBe('x'.repeat(count) + 'LIVE');
    expect(f.api.acknowledgeTerminal).toHaveBeenCalledWith({ tabId: f.input.tabId, generation: f.input.generation, sequence: count });
    expect(f.api.acknowledgeTerminal).toHaveBeenLastCalledWith({ tabId: f.input.tabId, generation: f.input.generation, sequence: count + 1 });
    expect(f.api.attachTerminal).toHaveBeenCalledTimes(1); expect(f.api.detachTerminal).not.toHaveBeenCalled();
    expect(f.registry.getState(f.input.tabId)).toMatchObject({ phase: 'open', error: null });
    expect(f.factory).toHaveBeenCalledTimes(1); expect(f.processes[0]!.kill).not.toHaveBeenCalled();
  });

  it('consumes a valid replay at both the 8192-chunk and 256 KiB UTF-8 bounds', async () => {
    const f = await fixture(); const part = '界'.repeat(10) + 'xy'; // 32 UTF-8 bytes per chunk
    for (let i = 0; i < 8192; i++) f.processes[0]!.output(part);
    const replay = value(f.backend.attach({ tabId: f.input.tabId }));
    expect(replay.chunks).toHaveLength(8192); expect(replay.truncated).toBe(false);
    f.registry.mount(f.input.tabId, f.host, true); await settle(); await f.parseAll();
    expect(f.latest().output).toBe(part.repeat(8192));
    expect(f.api.acknowledgeTerminal).toHaveBeenLastCalledWith({ tabId: f.input.tabId, generation: f.input.generation, sequence: 8192 });
    expect(f.api.attachTerminal).toHaveBeenCalledTimes(1); expect(f.api.detachTerminal).not.toHaveBeenCalled();
    f.processes[0]!.output('LIVE'); await f.parseAll(); expect(f.latest().output.endsWith('LIVE')).toBe(true);
  });

  it('preserves a fully consumed screen and keyboard input across a clean same-generation reattach', async () => {
    const f = await fixture(); f.registry.mount(f.input.tabId, f.host, true); await settle();
    f.processes[0]!.output('original'); await f.parseAll(); const screen = f.latest();
    f.registry.refresh(f.input.tabId); await settle();
    expect(f.latest()).toBe(screen); expect(screen.disposed).toBe(false);
    expect(f.api.attachTerminal).toHaveBeenLastCalledWith({ tabId: f.input.tabId, generation: f.input.generation, afterSequence: 1 });
    screen.input('user input'); await settle(); expect(f.processes[0]!.write).toHaveBeenCalledWith('user input');
    f.processes[0]!.output(' resumed'); await f.parseAll(); expect(screen.output).toBe('original resumed');
    expect(f.registry.getState(f.input.tabId)).toMatchObject({ phase: 'open', error: null });
  });

  it('automatically recovers a 1 MiB live burst through detach/full replay, without manual refresh', async () => {
    const f = await fixture(); f.registry.mount(f.input.tabId, f.host, true); await settle(); const old = f.latest();
    f.processes[0]!.output('x'.repeat(1024 * 1024)); await settle();
    expect(f.delivered.some(event => event.type === 'error')).toBe(true);
    expect(old.disposed).toBe(true);
    old.callbacks.forEach(done => done()); old.input('stale parser reply'); await settle();
    expect(f.api.acknowledgeTerminal).not.toHaveBeenCalled(); expect(f.api.writeTerminal).not.toHaveBeenCalled();
    expect(f.trace.slice(0, 3)).toEqual(['attach', 'detach', 'attach']);
    await f.parseAll();
    expect(f.latest().output).toBe('x'.repeat(256 * 1024));
    expect(f.api.acknowledgeTerminal).toHaveBeenLastCalledWith({ tabId: f.input.tabId, generation: f.input.generation, sequence: 256 });
    f.processes[0]!.output('AFTER-RECOVERY'); await f.parseAll();
    expect(f.latest().output.endsWith('AFTER-RECOVERY')).toBe(true);
    expect(f.api.acknowledgeTerminal).toHaveBeenLastCalledWith({ tabId: f.input.tabId, generation: f.input.generation, sequence: 257 });
    expect(f.registry.getState(f.input.tabId)).toMatchObject({ phase: 'open', error: null });
    expect(f.registry.getState(f.input.tabId).warning).toMatch(/Earlier output is unavailable/);
    expect(f.api.attachTerminal).toHaveBeenCalledTimes(2); expect(f.api.closeTab).not.toHaveBeenCalled();
  });

  it('serializes an in-flight old-view ACK before detach and fences its late failure and parser callbacks', async () => {
    const f = await fixture(); f.registry.mount(f.input.tabId, f.host, true); await settle();
    const pending = deferred<void>(); f.delayNextAck(() => pending.promise);
    f.processes[0]!.output('prefix'); const old = f.latest(); old.callbacks.shift()!(); await settle();
    expect(f.trace).toEqual(['attach', 'ack:1']);
    f.processes[0]!.output('x'.repeat(1024 * 1024)); await settle();
    expect(old.disposed).toBe(true); expect(f.api.detachTerminal).not.toHaveBeenCalled();
    expect(f.api.attachTerminal).toHaveBeenCalledTimes(1);
    old.callbacks.forEach(done => done()); old.input('old parser reply'); await settle();
    expect(f.api.acknowledgeTerminal).toHaveBeenCalledTimes(1); expect(f.api.writeTerminal).not.toHaveBeenCalled();
    pending.resolve(); await settle();
    expect(f.trace.slice(0, 5)).toEqual(['attach', 'ack:1', 'old-ack-complete', 'detach', 'attach']);
    await f.parseAll(); f.processes[0]!.output('LIVE-AGAIN'); await f.parseAll();
    expect(f.latest().output.endsWith('LIVE-AGAIN')).toBe(true);
    expect(f.registry.getState(f.input.tabId)).toMatchObject({ phase: 'open', error: null });
    expect(f.api.acknowledgeTerminal).toHaveBeenLastCalledWith({ tabId: f.input.tabId, generation: f.input.generation, sequence: 258 });
    expect(f.api.attachTerminal).toHaveBeenCalledTimes(2);
  });

  it('manual refresh with outstanding replay detaches before attaching and invalidates the discarded parser', async () => {
    const f = await fixture(); for (let i = 0; i < 3000; i++) f.processes[0]!.output('x');
    f.registry.mount(f.input.tabId, f.host, true); await settle(); const old = f.latest();
    f.registry.refresh(f.input.tabId); await settle();
    expect(f.trace).toEqual(['attach', 'detach', 'attach']);
    old.callbacks.forEach(done => done()); await settle(); expect(f.api.acknowledgeTerminal).not.toHaveBeenCalled();
    await f.parseAll(); expect(f.latest().output).toBe('x'.repeat(3000));
    f.processes[0]!.output('LIVE'); await f.parseAll(); expect(f.latest().output.endsWith('LIVE')).toBe(true);
    expect(f.registry.getState(f.input.tabId)).toMatchObject({ phase: 'open', error: null });
  });
});
