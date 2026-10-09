// @vitest-environment jsdom
import './terminal-test-mocks';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { Terminal } from '@xterm/headless';
import type { Result, TerminalApi, TerminalEvent } from '../shared/contracts';
import { success } from '../shared/contracts';
import { PtyBackend } from '../main/terminal/backend';
import { TerminalDelivery } from '../main/terminal/delivery';
import { tuiStream, tuiUpdate } from '../main/terminal/tui-fixture';
import { factoryFixture, launchInput } from '../main/terminal/test-fixtures';
import { TerminalRegistry, type SurfaceFactory } from './terminal-client';

// Real production PtyBackend (replay ring + screen mirror), TerminalDelivery and TerminalRegistry.
// Only the PTY process is a double; the renderer surface is a real xterm parser (headless).
const value = <T>(result: Result<T>): T => { if (!result.ok) throw new Error(result.error.code); return result.value; };
const tick = () => new Promise<void>(resolve => setTimeout(resolve, 0));
async function until(done: () => boolean) { for (let i = 0; i < 4000 && !done(); i++) await tick(); if (!done()) throw new Error('condition not reached'); }
const cleanups: (() => Promise<void>)[] = [];
afterEach(async () => { for (const cleanup of cleanups.splice(0)) await cleanup(); document.body.replaceChildren(); });

const VIEW = { cols: 120, rows: 30 };
function rowsOf(term: Terminal) { const b = term.buffer.active; return Array.from({ length: term.rows }, (_, y) => b.getLine(b.viewportY + y)?.translateToString(true) ?? ''); }
const idleRows = (rows: string[]) => rows.filter(row => /agent \d+ idle/.test(row)).length;

async function fixture() {
  const pty = factoryFixture(); const backend = new PtyBackend(pty.options);
  value(await backend.initialize()); const input = launchInput(); value(await backend.launch(input));
  const listeners = new Set<(event: TerminalEvent) => void>();
  const delivery = new TerminalDelivery(args => backend.attach(args), event => { for (const listener of listeners) listener(event); });
  const off = backend.subscribe(event => delivery.event(event));
  const terminals: Terminal[] = []; const sizes: string[] = [];
  const disposed = new Set<Terminal>(); const viewport = { ...VIEW };
  const factory: SurfaceFactory = () => {
    const term = new Terminal({ cols: 80, rows: 24, scrollback: 3000, allowProposedApi: true }); terminals.push(term);
    return {
      element: document.createElement('div'), write: (data, done) => term.write(data, done), reset: () => term.reset(),
      fit: () => { if (term.cols !== viewport.cols || term.rows !== viewport.rows) term.resize(viewport.cols, viewport.rows); return { ...viewport }; }, resize: (cols, rows) => { sizes.push(`${cols}x${rows}`); term.resize(cols, rows); },
      focus() {}, setInput() {}, dispose: () => { disposed.add(term); term.dispose(); },
    };
  };
  const api: TerminalApi = {
    getTerminalProfiles: async () => success(backend.getProfiles()),
    subscribeTerminal(listener) { listeners.add(listener); return () => { listeners.delete(listener); }; },
    attachTerminal: vi.fn(async args => { const replay = backend.attach(args); if (!replay.ok) return replay; const registered = delivery.attached(replay.value); return registered.ok ? replay : registered; }),
    acknowledgeTerminal: async args => delivery.acknowledge(args),
    detachTerminal: async args => delivery.detach(args),
    writeTerminal: async args => backend.write(args),
    resizeTerminal: vi.fn(async args => backend.resize(args)),
    closeTab: async () => { throw new Error('must not close'); },
  };
  const registry = new TerminalRegistry(api, factory); registry.start();
  const reference = new Terminal({ ...VIEW, scrollback: 3000, allowProposedApi: true });
  cleanups.push(async () => { registry.dispose(); reference.dispose(); off(); delivery.dispose(); await backend.dispose(); });
  const host = document.createElement('div'); document.body.append(host);
  const pump = async (chunks: string[]) => {
    for (let i = 0; i < chunks.length; i++) {
      const chunk = chunks[i]!;
      pty.processes[0]!.output(chunk);
      // Keep every individual PTY update, but yield on actual parser completion
      // in small batches. Per-chunk setTimeout(0) costs about 15 ms on Windows.
      if ((i + 1) % 32 === 0 || i === chunks.length - 1) await new Promise<void>(resolve => reference.write(chunk, resolve));
      else reference.write(chunk);
    }
  };
  return { ...pty, backend, api, registry, host, input, terminals, disposed, viewport, sizes, reference, pump, latest: () => terminals.at(-1)! };
}

describe('TUI screen restoration after output the replay ring no longer holds', () => {
  for (const alt of [false, true]) it(`keeps every row of a diff-rendered TUI across a tab switch with heavy background output (${alt ? 'alternate' : 'normal'} buffer)`, async () => {
    const f = await fixture(); const options = { ...VIEW, alt };
    const unmount = f.registry.mount(f.input.tabId, f.host, true);
    await until(() => f.registry.getState(f.input.tabId).phase === 'open'); await until(() => f.backend.get(f.input.tabId)?.cols === VIEW.cols);
    const stream = tuiStream({ ...options, frames: 1500 });
    await f.pump(stream.slice(0, 700));
    await until(() => f.registry.getState(f.input.tabId).phase === 'open' && rowsOf(f.latest()).some(r => r.includes('Working 698')));
    expect(idleRows(rowsOf(f.latest()))).toBeGreaterThan(20);

    // Switch away; the TUI keeps updating far beyond the 256 KiB replay window.
    unmount(); await tick();
    await f.pump(stream.slice(700)); const extra = Array.from({ length: 1500 }, (_, i) => tuiUpdate(options, 2000 + i));
    await f.pump(extra);
    const attached = value(f.backend.attach({ tabId: f.input.tabId, generation: f.input.generation, afterSequence: 700 }));
    expect(attached).toMatchObject({ truncated: true, snapshot: true });

    f.registry.mount(f.input.tabId, f.host, true);
    await until(() => f.registry.getState(f.input.tabId).phase === 'open' && rowsOf(f.latest()).some(r => r.includes('Working 3499')));
    await tick(); await tick();
    expect(idleRows(rowsOf(f.latest()))).toBeGreaterThan(20);
    expect(rowsOf(f.latest())).toEqual(rowsOf(f.reference));
    expect(f.registry.getState(f.input.tabId).warning).toBeNull();
    // The snapshot is parsed at the size it was taken at, and no repaint nudge was needed.
    expect(f.sizes).toContain(`${VIEW.cols}x${VIEW.rows}`);
    expect(f.api.resizeTerminal).not.toHaveBeenCalledWith(expect.objectContaining({ rows: VIEW.rows - 1 }));
    // Live output keeps landing on the restored screen.
    await f.pump([tuiUpdate(options, 77)]);
    await until(() => rowsOf(f.latest()).join('\n') === rowsOf(f.reference).join('\n'));
  }, 30000);

  it('restores an evicted TUI after visiting 39 other running terminals with only 24 retained screens', async () => {
    const f = await fixture(); const options = { ...VIEW };
    const unmount = f.registry.mount(f.input.tabId, f.host, true);
    await until(() => f.backend.get(f.input.tabId)?.cols === VIEW.cols);
    await f.pump(tuiStream({ ...options, frames: 20 }));
    await until(() => rowsOf(f.latest()).some(row => row.includes('Working 19')));
    const first = f.latest(); unmount(); await tick();
    await f.pump(Array.from({ length: 2000 }, (_, i) => tuiUpdate(options, 100 + i)));
    for (let i = 1; i < 40; i++) {
      const input = launchInput(); value(await f.backend.launch(input));
      const leave = f.registry.mount(input.tabId, f.host, true);
      await until(() => f.backend.get(input.tabId)?.cols === VIEW.cols);
      f.processes[i].output(`screen ${i}`);
      await until(() => rowsOf(f.latest()).some(row => row.includes(`screen ${i}`)));
      leave(); await tick();
    }
    expect(f.disposed.has(first)).toBe(true);
    expect(f.terminals.filter(term => !f.disposed.has(term))).toHaveLength(24);
    expect(f.backend.live()).toHaveLength(40); expect(f.factory).toHaveBeenCalledTimes(40);
    f.registry.mount(f.input.tabId, f.host, true);
    await until(() => rowsOf(f.latest()).join('\n') === rowsOf(f.reference).join('\n'));
    expect(f.registry.getState(f.input.tabId).warning).toBeNull();
    expect(f.terminals.filter(term => !f.disposed.has(term))).toHaveLength(24);
    expect(f.processes.every(pty => !pty.kill.mock.calls.length)).toBe(true);
    expect(f.factory).toHaveBeenCalledTimes(40);
  }, 30000);

  it('parses a retained snapshot at its old grid before fitting to a changed viewport', async () => {
    const f = await fixture(); const options = { ...VIEW, alt: true };
    const unmount = f.registry.mount(f.input.tabId, f.host, true);
    await until(() => f.backend.get(f.input.tabId)?.cols === VIEW.cols);
    await f.pump(tuiStream({ ...options, frames: 20 }));
    await until(() => rowsOf(f.latest()).some(row => row.includes('Working 19')));
    unmount(); await tick();
    await f.pump(Array.from({ length: 2000 }, (_, i) => tuiUpdate(options, 100 + i)));
    f.viewport.cols = 100; f.viewport.rows = 26; f.reference.resize(100, 26);
    f.registry.mount(f.input.tabId, f.host, true);
    await until(() => f.latest().cols === 100 && f.latest().rows === 26);
    expect(f.sizes.at(-1)).toBe('120x30');
    expect(rowsOf(f.latest())).toEqual(rowsOf(f.reference));
    expect(f.registry.getState(f.input.tabId).warning).toBeNull();
  }, 30000);

  it('recovers a live burst that overruns the view by restoring the screen, not a tail', async () => {
    const f = await fixture(); const options = { ...VIEW };
    f.registry.mount(f.input.tabId, f.host, true);
    await until(() => f.registry.getState(f.input.tabId).phase === 'open');
    await f.pump(tuiStream({ ...options, frames: 50 }));
    await until(() => rowsOf(f.latest()).some(r => r.includes('Working 49')));
    // A burst far above the delivery credit, then idle time for the parsers to settle.
    const burst = Array.from({ length: 4000 }, (_, i) => tuiUpdate(options, 100 + i)).join('');
    f.processes[0]!.output(burst); f.reference.write(burst);
    await until(() => rowsOf(f.latest()).some(r => r.includes('Working 4099')) && rowsOf(f.reference).some(r => r.includes('Working 4099')), );
    await tick(); await tick();
    expect(idleRows(rowsOf(f.latest()))).toBeGreaterThan(20);
    expect(rowsOf(f.latest())).toEqual(rowsOf(f.reference));
  }, 30000);
});
