import { afterEach, describe, expect, it, vi } from 'vitest';
import { success } from '../shared/contracts';
import { request } from './api';
import { deferred } from './test-fixtures';
import type { Result } from '../shared/contracts';

afterEach(() => vi.useRealTimers());

describe('renderer request boundary', () => {
  it('returns genuine result errors unchanged', async () => {
    const result: Result<never> = { ok: false, error: { code: 'FOCUS_DENIED', message: 'Switch manually.', retryable: true } };
    expect(await request(async () => result)).toBe(result);
  });
  it('does not expose thrown internal details', async () => {
    const result = await request(async () => { throw new Error('internal credential details'); });
    expect(result.ok).toBe(false);
    if (!result.ok) expect(result.error.message).not.toContain('credential');
  });
  it('bounds an IPC wait without repeating or canceling the original operation', async () => {
    vi.useFakeTimers();
    const late = deferred<Result<number>>();
    const operation = vi.fn(() => late.promise);
    const result = request(operation, 100);
    await vi.advanceTimersByTimeAsync(100);
    expect(await result).toMatchObject({ ok: false, error: { retryable: false } });
    expect(operation).toHaveBeenCalledTimes(1);
    late.resolve(success(42));
    expect(await result).toMatchObject({ ok: false });
    expect(vi.getTimerCount()).toBe(0);
  });
  it('leaves native directory pickers unbounded and clears completed request timers', async () => {
    vi.useFakeTimers();
    expect(await request(async () => success(null), 0)).toEqual(success(null));
    expect(await request(async () => success(42), 100)).toEqual(success(42));
    expect(vi.getTimerCount()).toBe(0);
  });
});
