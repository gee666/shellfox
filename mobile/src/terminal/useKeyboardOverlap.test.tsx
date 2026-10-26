// @vitest-environment jsdom
import { act, cleanup, renderHook } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { useKeyboardOverlap } from './useKeyboardOverlap';

const native = vi.hoisted(() => ({
  platform: { OS: 'android' }, visible: false, top: 500,
  listeners: new Map<string, (event: { endCoordinates: { screenY: number } }) => void>(),
  remove: vi.fn(),
}));
vi.mock('react-native', () => ({
  Platform: native.platform,
  Keyboard: {
    isVisible: () => native.visible,
    metrics: () => native.visible ? { screenY: native.top } : undefined,
    addListener: (type: string, listener: (event: { endCoordinates: { screenY: number } }) => void) => {
      native.listeners.set(type, listener);
      return { remove: () => { native.remove(); native.listeners.delete(type); } };
    },
  },
}));
function event(visible: boolean) {
  act(() => {
    native.visible = visible;
    native.listeners.get(visible ? 'keyboardDidShow' : 'keyboardDidHide')?.({ endCoordinates: { screenY: native.top } });
  });
}
function fixture(y: number, height: number) {
  // The hook needs only measurement, not a mounted native renderer.
  const measure = vi.fn((callback: (x: number, y: number, width: number, height: number) => void) => callback(0, y, 390, height));
  const ref = { current: { measureInWindow: measure } };
  return renderHook(() => useKeyboardOverlap(ref));
}
beforeEach(() => { vi.clearAllMocks(); native.platform.OS = 'android'; native.visible = false; native.top = 500; });
afterEach(cleanup);

describe('keyboard overlap measurement', () => {
  it('uses only Android edge-to-edge overlap and clears it when the IME hides', () => {
    const { result } = fixture(30, 770);
    expect(result.current.keyboardInset).toBe(0);
    event(true);
    expect(result.current.keyboardVisible).toBe(true);
    expect(result.current.keyboardInset).toBe(300);
    event(false);
    expect(result.current.keyboardVisible).toBe(false);
    expect(result.current.keyboardInset).toBe(0);
  });
  it('does not double-subtract already-resized Android parents', () => {
    const { result } = fixture(30, 470);
    event(true);
    expect(result.current.keyboardInset).toBe(0);
  });
  it('lets iOS KeyboardAvoidingView handle height, using the actual root offset', () => {
    native.platform.OS = 'ios';
    const { result, unmount } = fixture(62, 738);
    event(true);
    expect(result.current.keyboardOffset).toBe(62);
    expect(result.current.keyboardInset).toBe(0);
    unmount();
    expect(native.remove).toHaveBeenCalledTimes(2);
  });
});
