// Read-only native layout diagnostics for explicit QA bundles. Nothing is rendered.
import { useEffect } from 'react';
import { PixelRatio, useWindowDimensions } from 'react-native';
import { useSafeAreaInsets } from 'react-native-safe-area-context';

export function FilesQaMetrics() {
  const { width, height } = useWindowDimensions();
  const insets = useSafeAreaInsets();
  useEffect(() => {
    console.info(`FILES_QA_VIEWPORT ${JSON.stringify({ width, height, scale: PixelRatio.get(), insets })}`);
  }, [width, height, insets.top, insets.right, insets.bottom, insets.left]);
  return null;
}
