// @vitest-environment jsdom
import { act, renderHook, cleanup } from '@testing-library/react';
import { afterEach, expect, it, vi } from 'vitest';
import { useStickyModifiers } from './useStickyModifiers';
import { emptyModifiers } from './protocol';

afterEach(() => { cleanup(); vi.useRealTimers(); });
it.each(['ctrl', 'alt', 'shift', 'meta', 'capsLock'] as const)('keeps %s armed until input, then releases 430 ms after the last key', key => {
  vi.useFakeTimers();
  const send = vi.fn();
  const { result } = renderHook(() => useStickyModifiers(send));
  act(() => result.current.toggle(key));
  act(() => vi.advanceTimersByTime(60_000));
  expect(result.current.modifiers[key]).toBe(true);
  act(() => result.current.activity());
  act(() => vi.advanceTimersByTime(400));
  expect(result.current.modifiers[key]).toBe(true);
  act(() => result.current.activity());
  act(() => vi.advanceTimersByTime(429));
  expect(result.current.modifiers[key]).toBe(true);
  act(() => vi.advanceTimersByTime(1));
  expect(result.current.modifiers).toEqual(emptyModifiers);
  expect(send).toHaveBeenLastCalledWith(emptyModifiers);
});
it('toggles off immediately and arms a new chord without the old timeout', () => {
  vi.useFakeTimers();
  const { result } = renderHook(() => useStickyModifiers(vi.fn()));
  act(() => result.current.toggle('ctrl'));
  act(() => result.current.toggle('ctrl'));
  expect(result.current.modifiers.ctrl).toBe(false);
  act(() => result.current.toggle('alt'));
  act(() => result.current.activity());
  act(() => result.current.toggle('shift'));
  act(() => vi.advanceTimersByTime(5000));
  expect(result.current.modifiers).toEqual({ ...emptyModifiers, alt: true, shift: true });
  act(() => result.current.reset());
  expect(result.current.modifiers).toEqual(emptyModifiers);
});
it('cancels pending releases on reset and unmount', () => {
  vi.useFakeTimers();
  const send = vi.fn();
  const { result, unmount } = renderHook(() => useStickyModifiers(send));
  act(() => result.current.toggle('meta'));
  act(() => result.current.activity());
  act(() => result.current.reset());
  expect(result.current.modifiers).toEqual(emptyModifiers);
  send.mockClear();
  act(() => vi.runAllTimers());
  expect(send).not.toHaveBeenCalled();
  act(() => result.current.toggle('ctrl'));
  act(() => result.current.activity());
  unmount(); send.mockClear();
  act(() => vi.runAllTimers());
  expect(send).not.toHaveBeenCalled();
});
it('uses the latest callback without restarting a pending release on rerender', () => {
  vi.useFakeTimers();
  const first = vi.fn(), next = vi.fn();
  const { result, rerender } = renderHook(({ send }) => useStickyModifiers(send), { initialProps: { send: first } });
  act(() => result.current.toggle('ctrl'));
  act(() => result.current.activity());
  act(() => vi.advanceTimersByTime(400));
  rerender({ send: next });
  act(() => vi.advanceTimersByTime(29));
  expect(result.current.modifiers.ctrl).toBe(true);
  act(() => vi.advanceTimersByTime(1));
  expect(result.current.modifiers).toEqual(emptyModifiers);
  expect(first).toHaveBeenCalledExactlyOnceWith({ ...emptyModifiers, ctrl: true });
  expect(next).toHaveBeenCalledExactlyOnceWith(emptyModifiers);
});
it('does not schedule a release without armed modifiers', () => {
  vi.useFakeTimers();
  const send = vi.fn();
  const { result } = renderHook(() => useStickyModifiers(send));
  act(() => result.current.activity());
  expect(vi.getTimerCount()).toBe(0);
  expect(send).not.toHaveBeenCalled();
});
