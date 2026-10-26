import { useCallback, useEffect, useState, type ComponentRef, type RefObject } from 'react';
import { Keyboard, Platform, type View } from 'react-native';

/** Measure only the unhandled overlap, including Android edge-to-edge windows. */
export function useKeyboardOverlap(container: RefObject<Pick<ComponentRef<typeof View>, 'measureInWindow'> | null>) {
  const [keyboardInset, setKeyboardInset] = useState(0);
  const [keyboardOffset, setKeyboardOffset] = useState(0);
  const [keyboardVisible, setKeyboardVisible] = useState(Keyboard.isVisible());
  const measureKeyboardInset = useCallback((top = Keyboard.metrics()?.screenY) => {
    container.current?.measureInWindow((_x, y, _width, height) => {
      if (Platform.OS === 'ios') { setKeyboardOffset(y); return; }
      // Already-resized parents must not lose height twice.
      setKeyboardInset(top === undefined || !Keyboard.isVisible() ? 0 : Math.max(0, y + height - top));
    });
  }, [container]);
  useEffect(() => {
    measureKeyboardInset();
    const show = Keyboard.addListener('keyboardDidShow', event => {
      setKeyboardVisible(true);
      measureKeyboardInset(event.endCoordinates.screenY);
    });
    const hide = Keyboard.addListener('keyboardDidHide', () => {
      setKeyboardVisible(false);
      setKeyboardInset(0);
    });
    return () => { show.remove(); hide.remove(); };
  }, [measureKeyboardInset]);
  return { keyboardVisible, keyboardInset, keyboardOffset, measureKeyboardInset };
}
