import React, { type ReactNode } from 'react';
import { Pressable, ScrollView, StyleSheet, Text, View, type StyleProp, type ViewStyle } from 'react-native';
import { useTheme } from '../theme';

interface ContainerProps {
  children: ReactNode;
  bottomInset?: number;
  style?: StyleProp<ViewStyle>;
}

/** Shared row for terminal controls and keyboard-adjacent suggestions. */
export function KeyboardAccessory({ children, bottomInset = 0, style }: ContainerProps) {
  const { colors } = useTheme();
  return <View style={[styles.row, { backgroundColor: colors.panel, borderColor: colors.line,
    paddingBottom: Math.max(4, bottomInset) }, style]}>{children}</View>;
}

export function KeyboardAccessoryScroll({ children }: { children: ReactNode }) {
  return <ScrollView horizontal style={styles.scroll} contentContainerStyle={styles.keys}
    showsHorizontalScrollIndicator={false} keyboardShouldPersistTaps="always">{children}</ScrollView>;
}

interface ButtonProps {
  label: string;
  onPress: () => void;
  active?: boolean;
  disabled?: boolean;
  children?: ReactNode;
}

export function AccessoryButton({ label, onPress, active = false, disabled = false, children }: ButtonProps) {
  const { colors } = useTheme();
  return <Pressable accessibilityRole="button" accessibilityLabel={label}
    accessibilityState={{ selected: active, disabled }} onPress={onPress} disabled={disabled}
    style={({ pressed }) => [styles.key, { backgroundColor: active ? colors.accent : colors.bg,
      borderColor: active ? colors.accent : colors.line }, disabled && styles.disabled, pressed && styles.pressed]}>
    {children ?? <Text style={[styles.label, { color: active ? colors.onAccent : colors.text }]}>{label}</Text>}
  </Pressable>;
}
const styles = StyleSheet.create({
  row: { flexDirection: 'row', gap: 4, paddingHorizontal: 4, borderTopWidth: 1, paddingTop: 4 },
  scroll: { flex: 1 },
  keys: { gap: 4 },
  key: { borderWidth: 1, borderRadius: 3, minWidth: 44, minHeight: 44, paddingHorizontal: 9,
    justifyContent: 'center', alignItems: 'center' },
  label: { fontSize: 14 },
  disabled: { opacity: 0.4 },
  pressed: { opacity: 0.65 },
});
