// @vitest-environment jsdom
import { act, renderHook, cleanup } from '@testing-library/react';
import { afterEach, expect, it, vi } from 'vitest';
import { useStickyModifiers } from './useStickyModifiers';
import { emptyModifiers } from './protocol';

afterEach(() => { cleanup(); vi.useRealTimers(); });
it('waits indefinitely before input, then releases after the last key', () => {
  vi.useFakeTimers();
  const send = vi.fn();
  const { result } = renderHook(() => useStickyModifiers(1000, send));
  act(() => result.current.toggle('ctrl'));
  act(() => vi.advanceTimersByTime(60_000));
  expect(result.current.modifiers.ctrl).toBe(true);
  act(() => result.current.activity());
  act(() => vi.advanceTimersByTime(900));
  expect(result.current.modifiers.ctrl).toBe(true);
  act(() => result.current.activity());
  act(() => vi.advanceTimersByTime(999));
  expect(result.current.modifiers.ctrl).toBe(true);
  act(() => vi.advanceTimersByTime(1));
  expect(result.current.modifiers).toEqual(emptyModifiers);
  expect(send).toHaveBeenLastCalledWith(emptyModifiers);
});
it('toggles off immediately and arms a new chord without the old timeout', () => {
  vi.useFakeTimers();
  const { result } = renderHook(() => useStickyModifiers(1000, vi.fn()));
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
it('uses the configured delay and cancels pending releases on unmount', () => {
  vi.useFakeTimers();
  const send = vi.fn();
  const { result, unmount } = renderHook(() => useStickyModifiers(250, send));
  act(() => result.current.toggle('meta'));
  act(() => result.current.activity());
  act(() => vi.advanceTimersByTime(249));
  expect(result.current.modifiers.meta).toBe(true);
  act(() => vi.advanceTimersByTime(1));
  expect(result.current.modifiers.meta).toBe(false);
  act(() => result.current.toggle('ctrl'));
  act(() => result.current.activity());
  unmount(); send.mockClear();
  act(() => vi.runAllTimers());
  expect(send).not.toHaveBeenCalled();
});
