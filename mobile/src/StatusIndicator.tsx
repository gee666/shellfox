import React, { useEffect, useRef } from 'react';
import { Animated, Easing, View } from 'react-native';
import type { DotState } from '../../src/shared/terminal-status';
import { useTheme } from './theme';

export function StatusIndicator({
  kind = 'shell', loading = false, label = 'Shell',
}: {
  kind?: DotState['kind'];
  loading?: boolean;
  label?: string;
}) {
  const { colors } = useTheme();
  const statusColors: Record<DotState['kind'], string> = {
    shell: colors.shell, running: colors.green, busy: colors.green, settled: colors.settled, error: colors.danger,
  };
  const progress = useRef(new Animated.Value(0)).current;
  const pulsing = !loading && kind === 'busy';
  useEffect(() => {
    progress.setValue(0);
    if (!loading && !pulsing) return;
    const animation = Animated.loop(loading
      ? Animated.timing(progress, { toValue: 1, duration: 900, easing: Easing.linear, useNativeDriver: true, isInteraction: false })
      : Animated.sequence([
        Animated.timing(progress, { toValue: 1, duration: 600, easing: Easing.inOut(Easing.ease), useNativeDriver: true, isInteraction: false }),
        Animated.timing(progress, { toValue: 0, duration: 600, easing: Easing.inOut(Easing.ease), useNativeDriver: true, isInteraction: false }),
      ]));
    animation.start();
    return () => animation.stop();
  }, [loading, pulsing, progress]);

  return (
    <View
      accessible
      accessibilityRole={loading ? 'progressbar' : 'image'}
      accessibilityLabel={loading && label === 'Shell' ? 'Loading' : label}
      accessibilityState={{ busy: loading }}
      style={{ flexDirection: 'row', alignItems: 'center', gap: 3 }}
    >
      {loading ? [0, 1, 2].map(index => (
        <Animated.View key={index} style={{
          width: 4, height: 4, borderRadius: 2, backgroundColor: statusColors.shell,
          opacity: progress.interpolate({ inputRange: [0, 1 / 3, 2 / 3, 1], outputRange: index === 0 ? [1, 0.25, 0.25, 1] : index === 1 ? [0.25, 1, 0.25, 0.25] : [0.25, 0.25, 1, 0.25] }),
        }} />
      )) : (
        <Animated.View style={{
          width: 8, height: 8, borderRadius: 4, backgroundColor: statusColors[kind],
          opacity: pulsing ? progress.interpolate({ inputRange: [0, 1], outputRange: [1, 0.45] }) : 1,
          transform: [{ scale: pulsing ? progress.interpolate({ inputRange: [0, 1], outputRange: [1, 0.85] }) : 1 }],
        }} />
      )}
    </View>
  );
}
