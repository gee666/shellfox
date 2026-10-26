import React from 'react';
import { ActivityIndicator, Pressable, StyleSheet, Text, TextInput, View } from 'react-native';
import type { TextInputProps, ViewStyle } from 'react-native';

export const colors = {
  bg: '#111318',
  panel: '#1b1e25',
  line: '#30343d',
  text: '#e9eaf0',
  muted: '#9bA1af',
  accent: '#bcafff',
  danger: '#ffa6a6',
  green: '#a6d6ad',
};
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
  return (
    <View style={styles.field}>
      <Text style={styles.label}>{label}</Text>
      <TextInput
        {...props}
        accessibilityLabel={label}
        placeholderTextColor={colors.muted}
        style={[styles.input, props.style]}
      />
    </View>
  );
}
export function Note({ children, error = false }: { children: React.ReactNode; error?: boolean }) {
  return (
    <Text accessibilityLiveRegion="polite" style={[styles.note, error && { color: colors.danger }]}>
      {children}
    </Text>
  );
}
export function Busy() {
  return <ActivityIndicator color={colors.accent} style={{ padding: 20 }} />;
}
export function Card({ children, style }: { children: React.ReactNode; style?: ViewStyle }) {
  return <View style={[styles.card, style]}>{children}</View>;
}
export const styles = StyleSheet.create({
  page: { flex: 1, backgroundColor: colors.bg },
  content: { padding: 18, gap: 16, paddingBottom: 30 },
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
  title: { fontSize: 24, fontWeight: '600', color: colors.text },
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
    padding: 16,
    gap: 12,
    backgroundColor: colors.panel,
    borderRadius: 10,
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
    borderRadius: 7,
  },
  subtle: { backgroundColor: '#282c35' },
  buttonText: { color: colors.bg, fontSize: 14, fontWeight: '600' },
  field: { gap: 7 },
  input: {
    minHeight: 46,
    paddingHorizontal: 12,
    paddingVertical: 10,
    color: colors.text,
    backgroundColor: colors.bg,
    borderWidth: 1,
    borderColor: colors.line,
    borderRadius: 7,
    fontSize: 15,
  },
});
