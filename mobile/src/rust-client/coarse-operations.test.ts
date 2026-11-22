import { afterEach, describe, expect, it, vi } from 'vitest';
import { CoarseOperations } from './coarse-operations';

interface Event { operationId: string; status: number }

function bridge() {
  const queue: Event[] = [];
  let signal: (() => void) | undefined;
  return {
    queue,
    signal: () => signal?.(),
    native: {
      read: async () => queue.shift() ?? null,
      cancel: vi.fn(async () => undefined),
      close: vi.fn(async () => undefined),
      subscribe: (listener: () => void) => { signal = listener; return () => { signal = undefined; }; },
    },
  };
}

describe('CoarseOperations', () => {
  afterEach(() => { vi.useRealTimers(); });

  it('resolves on the native signal without any timer', async () => {
    vi.useFakeTimers();
    const b = bridge();
    const pump = new CoarseOperations<Event>(b.native, () => undefined, () => undefined);
    pump.start();
    const result = pump.invoke(async () => '7');
    await Promise.resolve(); await Promise.resolve();
    b.queue.push({ operationId: '7', status: 0 });
    b.signal();
    await expect(result).resolves.toEqual({ operationId: '7', status: 0 });
    expect(vi.getTimerCount()).toBe(0); // no pull timer, and the 35 s guard is cleared
    await pump.close();
  });

  it('keeps draining a long batch after yielding', async () => {
    const b = bridge();
    const seen: string[] = [];
    const pump = new CoarseOperations<Event>(b.native, event => seen.push(event.operationId), () => undefined);
    for (let i = 1; i <= 20; i++) b.queue.push({ operationId: String(i), status: 0 });
    pump.start();
    await vi.waitFor(() => expect(seen).toHaveLength(20));
    await pump.close();
    b.signal();
    expect(b.native.close).toHaveBeenCalledOnce();
  });
});
