import React, { useMemo } from 'react';
import { Pressable, StyleSheet, Text, TextInput, View } from 'react-native';
import type { TextInputProps, ViewStyle } from 'react-native';

import { useTheme, type ThemeColors } from './theme';

export function useUi() {
  const { colors } = useTheme();
  const styles = useMemo(() => createStyles(colors), [colors]);
  return { colors, styles };
}
export function Button({
  title,
  onPress,
  disabled = false,
  subtle = false,
  danger = false,
  testID,
}: {
  title: string;
  onPress: () => void;
  disabled?: boolean;
  subtle?: boolean;
  danger?: boolean;
  testID?: string;
}) {
  const { colors, styles } = useUi();
  return (
    <Pressable
      accessibilityRole="button"
      accessibilityLabel={title}
      accessibilityState={{ disabled }}
      testID={testID}
      disabled={disabled}
      onPress={onPress}
      style={({ pressed }) => [
        styles.button,
        subtle && styles.subtle,
        { opacity: disabled ? 0.4 : pressed ? 0.65 : 1 },
      ]}
    >
      <Text style={[styles.buttonText, subtle && { color: colors.text }, danger && { color: colors.danger }]}>
        {title}
      </Text>
    </Pressable>
  );
}
export function Field({ label, ...props }: TextInputProps & { label: string }) {
  const { colors, styles } = useUi();
  return (
    <View style={styles.field}>
      <Text style={styles.label}>{label}</Text>
      <TextInput
        {...props}
        accessibilityLabel={label}
        placeholderTextColor={colors.placeholder}
        style={[styles.input, props.style]}
      />
    </View>
  );
}
export function Note({ children, error = false }: { children: React.ReactNode; error?: boolean }) {
  const { colors, styles } = useUi();
  return (
    <Text accessibilityLiveRegion="polite" style={[styles.note, error && { color: colors.danger }]}>
      {children}
    </Text>
  );
}
export function Card({ children, style }: { children: React.ReactNode; style?: ViewStyle }) {
  const { styles } = useUi();
  return <View style={[styles.card, style]}>{children}</View>;
}
const createStyles = (colors: ThemeColors) => StyleSheet.create({
  page: { flex: 1, backgroundColor: colors.bg },
  content: { padding: 12, gap: 12, paddingBottom: 24 },
  header: {
    paddingHorizontal: 18,
    paddingVertical: 12,
    flexDirection: 'row',
    alignItems: 'center',
    justifyContent: 'space-between',
    gap: 10,
    borderBottomWidth: 1,
    borderColor: colors.line,
  },
  title: { fontSize: 20, fontWeight: '600', color: colors.text },
  subtitle: { fontSize: 13, color: colors.muted, marginTop: 3 },
  section: {
    fontSize: 12,
    letterSpacing: 1,
    fontWeight: '600',
    color: colors.muted,
    textTransform: 'uppercase',
  },
  row: { flexDirection: 'row', alignItems: 'center', gap: 10 },
  grow: { flex: 1 },
  card: {
    padding: 12,
    gap: 10,
    backgroundColor: colors.panel,
    borderRadius: 4,
    borderWidth: 1,
    borderColor: colors.line,
  },
  label: { color: colors.text, fontSize: 14, fontWeight: '500' },
  note: { color: colors.muted, fontSize: 13, lineHeight: 19 },
  button: {
    minHeight: 44,
    paddingHorizontal: 15,
    justifyContent: 'center',
    alignItems: 'center',
    backgroundColor: colors.accent,
    borderRadius: 4,
  },
  subtle: { backgroundColor: colors.surface },
  buttonText: { color: colors.onAccent, fontSize: 14, fontWeight: '600' },
  field: { gap: 7 },
  input: {
    minHeight: 46,
    paddingHorizontal: 12,
    paddingVertical: 10,
    color: colors.text,
    backgroundColor: colors.input,
    borderWidth: 1,
    borderColor: colors.inputBorder,
    borderRadius: 4,
    fontSize: 15,
  },
});
