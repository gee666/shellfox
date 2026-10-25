import { afterEach, expect, it, vi } from 'vitest';
import type { Result } from '../../shared/contracts';
import { PtyBackend, type PtyFactory } from './backend';
import { MAX_NATIVE_INPUT_BYTES } from './native-input';
import { factoryFixture, launchInput } from './test-fixtures';

const backends: PtyBackend[] = [];
afterEach(async () => { for (const backend of backends.splice(0)) await backend.dispose(); });
const value = <T>(result: Result<T>) => { if (!result.ok) throw new Error(result.error.message); return result.value; };
async function fixture() {
  const f = factoryFixture(); const backend = new PtyBackend(f.options); backends.push(backend);
  value(await backend.initialize()); const input = launchInput(); value(await backend.launch(input));
  return { ...f, backend, input, pty: f.processes[0]! };
}
it('answers DA/DSR from the persistent parser before any UI attaches, once per actual query', async () => {
  const f = await fixture();
  f.pty.output('\x1b[c\x1b[6n');
  expect(f.pty.write.mock.calls).toEqual([['\x1b[?1;2c'], ['\x1b[1;1R']]);
  const replay = value(f.backend.attach({ tabId: f.input.tabId }));
  expect(replay.chunks.map(chunk => chunk.data).join('')).toBe('\x1b[c\x1b[6n');
  value(f.backend.attach({ tabId: f.input.tabId }));
  expect(f.pty.write).toHaveBeenCalledTimes(2);
  f.pty.output('\x1b[6n'); expect(f.pty.write).toHaveBeenCalledTimes(3);
});
it.each([null, Number.NaN, -1, Number.POSITIVE_INFINITY, MAX_NATIVE_INPUT_BYTES - 1])('does not send device replies with an unbounded/full native queue=%s', async queued => {
  const f = await fixture(); f.pty.queuedInputBytes.mockReturnValue(queued);
  f.pty.output('\x1b[c'); expect(f.pty.write).not.toHaveBeenCalled();
  f.pty.queuedInputBytes.mockReturnValue(0);
  f.pty.output('\x1b[c'); expect(f.pty.write).toHaveBeenCalledExactlyOnceWith('\x1b[?1;2c');
});
it('counts device replies against the same per-second budget as user input', async () => {
  const f = await fixture(); const now = vi.spyOn(Date, 'now').mockReturnValue(Date.now());
  try {
    const write = (data: string) => f.backend.write({ tabId: f.input.tabId, generation: f.input.generation, data });
    f.pty.output('\x1b[c');
    for (let i = 0; i < 3; i++) value(write('x'.repeat(65536)));
    value(write('x'.repeat(65536 - 7)));
    expect(write('x')).toMatchObject({ ok: false, error: { code: 'VALIDATION' } });
    const calls = f.pty.write.mock.calls.length;
    f.pty.output('\x1b[c'.repeat(10000)); expect(f.pty.write).toHaveBeenCalledTimes(calls);
    now.mockReturnValue(Date.now() + 1001);
    f.pty.output('\x1b[c'); expect(f.pty.write).toHaveBeenCalledTimes(calls + 1);
  } finally { now.mockRestore(); }
});
it('can answer startup queries before launch readiness, but only after native input is inspectable', async () => {
  const f = factoryFixture(); let ready!: () => void;
  const waiting = new Promise<void>(resolve => { ready = resolve; });
  const factory: PtyFactory = (file, args, options) => {
    const pty = f.factory(file, args, options), onData = pty.onData.bind(pty);
    pty.whenReady = () => waiting;
    pty.onData = listener => { const off = onData(listener); listener('\x1b[c'); return off; };
    return pty;
  };
  const backend = new PtyBackend({ ...f.options, factory }); backends.push(backend);
  value(await backend.initialize()); const launch = backend.launch(launchInput());
  await expect.poll(() => f.processes[0]?.write.mock.calls.length).toBe(1);
  expect(f.processes[0]!.write).toHaveBeenCalledWith('\x1b[?1;2c');
  ready(); value(await launch);
});
it('does not turn reply failures into a broken screen or reply after shell exit', async () => {
  const f = await fixture(); f.pty.write.mockImplementationOnce(() => { throw new Error('Pipe closed'); });
  expect(() => f.pty.output('\x1b[c')).not.toThrow();
  expect(value(f.backend.attach({ tabId: f.input.tabId })).lastSequence).toBe(1);
  f.pty.exit(0); f.pty.output('\x1b[c'); expect(f.pty.write).toHaveBeenCalledOnce();
});
