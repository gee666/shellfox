// @vitest-environment jsdom
import React, { type ReactNode } from 'react';
import { cleanup, render } from '@testing-library/react';
import { afterEach, expect, it, vi } from 'vitest';
import { SwipeView, swipeDirection } from './SwipeView';

type Touch = { nativeEvent: { pageX: number; pageY: number; touches: object[] } };
interface Handlers {
  onTouchStart: (event: Touch) => void;
  onMoveShouldSetResponderCapture: (event: Touch) => boolean;
  onResponderGrant: () => boolean;
  onResponderRelease: (event: Touch) => void;
  onResponderTerminate: () => void;
}
let handlers: Handlers;
vi.mock('react-native', () => ({ View: (props: Handlers & { children: ReactNode }) => {
  handlers = props; return <div>{props.children}</div>;
} }));
afterEach(cleanup);
const touch = (x: number, y = 20, fingers = 1): Touch => ({ nativeEvent: { pageX: x, pageY: y, touches: Array.from({ length: fingers }, () => ({})) } });

it.each([[80, 10, 'right'], [-80, 10, 'left'], [20, 0, null], [70, 60, null], [0, 100, null]] as const)(
  'classifies displacement %i/%i as %s', (dx, dy, expected) => expect(swipeDirection(dx, dy)).toBe(expected),
);
it('switches only after a deliberate swipe release, not a tap or vertical scroll', () => {
  const onRight = vi.fn(), onLeft = vi.fn();
  render(<SwipeView onRight={onRight} onLeft={onLeft}>content</SwipeView>);
  handlers.onTouchStart(touch(10));
  expect(handlers.onMoveShouldSetResponderCapture(touch(20))).toBe(false);
  expect(handlers.onMoveShouldSetResponderCapture(touch(30, 160))).toBe(false);
  expect(handlers.onMoveShouldSetResponderCapture(touch(100))).toBe(true);
  expect(onRight).not.toHaveBeenCalled();
  expect(handlers.onResponderGrant()).toBe(true);
  handlers.onResponderRelease(touch(100)); expect(onRight).toHaveBeenCalledOnce();
  handlers.onTouchStart(touch(120)); handlers.onResponderRelease(touch(20));
  expect(onLeft).toHaveBeenCalledOnce();
});
it('claims horizontal movement early but does not switch on a short release', () => {
  const onRight = vi.fn(); render(<SwipeView onRight={onRight}>content</SwipeView>);
  handlers.onTouchStart(touch(10));
  expect(handlers.onMoveShouldSetResponderCapture(touch(30))).toBe(true);
  handlers.onResponderRelease(touch(40)); expect(onRight).not.toHaveBeenCalled();
});
it('ignores unavailable directions, cancelled gestures and multi-touch', () => {
  const onLeft = vi.fn(); render(<SwipeView onLeft={onLeft}>content</SwipeView>);
  handlers.onTouchStart(touch(10)); expect(handlers.onMoveShouldSetResponderCapture(touch(100))).toBe(false);
  handlers.onResponderTerminate(); handlers.onResponderRelease(touch(-100)); expect(onLeft).not.toHaveBeenCalled();
  handlers.onTouchStart(touch(120, 20, 2)); expect(handlers.onMoveShouldSetResponderCapture(touch(10))).toBe(false);
});
