import React, { useRef, type ReactNode } from 'react';
import { View, type GestureResponderEvent } from 'react-native';

export function swipeDirection(dx: number, dy: number, minimum = 60): 'left' | 'right' | null {
  return Math.abs(dx) >= minimum && Math.abs(dx) > Math.abs(dy) * 2 ? dx > 0 ? 'right' : 'left' : null;
}

/** Observe touches without taking vertical scrolling or taps away from children. */
export function SwipeView({ children, onRight, onLeft, fill = true }: { children: ReactNode; onRight?: () => void; onLeft?: () => void; fill?: boolean }) {
  const start = useRef<{ x: number; y: number } | null>(null);
  const direction = (event: GestureResponderEvent, minimum = 60) => {
    const touch = event.nativeEvent;
    return start.current ? swipeDirection(touch.pageX - start.current.x, touch.pageY - start.current.y, minimum) : null;
  };
  return <View style={fill ? { flex: 1 } : undefined}
    onTouchStart={event => { start.current = event.nativeEvent.touches.length === 1
      ? { x: event.nativeEvent.pageX, y: event.nativeEvent.pageY } : null; }}
    onMoveShouldSetResponderCapture={event => {
      const swipe = direction(event, 12);
      return swipe === 'right' ? !!onRight : swipe === 'left' && !!onLeft;
    }}
    onResponderGrant={() => true}
    onResponderRelease={event => {
      const swipe = direction(event); start.current = null;
      if (swipe === 'right') onRight?.();
      if (swipe === 'left') onLeft?.();
    }}
    onResponderTerminate={() => { start.current = null; }}>
    {children}
  </View>;
}
