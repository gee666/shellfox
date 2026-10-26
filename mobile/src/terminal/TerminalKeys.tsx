import React, { useState } from 'react';
import {
  KeyboardAvoidingView,
  Modal,
  Platform,
  Pressable,
  ScrollView,
  StyleSheet,
  Text,
  TextInput,
  View,
} from 'react-native';
import { colors } from '../ui';

interface Props {
  bottomInset: number;
  control: boolean;
  selecting: boolean;
  selection: string;
  onControl: () => void;
  onSelect: () => void;
  onKey: (data: string) => void;
  onKeyboard: () => void;
  onPaste: (text: string) => void;
}
const keys = [
  ['Esc', '\x1b'],
  ['Tab', '\t'],
  ['↑', '\x1b[A'],
  ['↓', '\x1b[B'],
  ['←', '\x1b[D'],
  ['→', '\x1b[C'],
] as const;
export function TerminalKeys(props: Props) {
  const [dialog, setDialog] = useState<'copy' | 'paste' | null>(null);
  const [paste, setPaste] = useState('');
  const button = (label: string, onPress: () => void, active = false, disabled = false) => (
    <Pressable
      key={label}
      onPress={onPress}
      disabled={disabled}
      accessibilityRole="button"
      accessibilityLabel={label}
      accessibilityState={{ selected: active, disabled }}
      style={({ pressed }) => [
        styles.key,
        active && styles.active,
        disabled && styles.disabled,
        pressed && styles.pressed,
      ]}
    >
      <Text style={[styles.label, active && styles.activeLabel]}>{label}</Text>
    </Pressable>
  );
  return (
    <>
      <View style={[styles.row, { paddingBottom: Math.max(4, props.bottomInset) }]}>
        <ScrollView
          horizontal
          style={styles.scroll}
          showsHorizontalScrollIndicator={false}
          keyboardShouldPersistTaps="always"
          contentContainerStyle={styles.keys}
        >
          {button('Ctrl', props.onControl, props.control)}
          {keys.map(([label, data]) => button(label, () => props.onKey(data)))}
          {button('Select', props.onSelect, props.selecting)}
          {button('Copy', () => setDialog('copy'), false, !props.selection)}
          {button('Paste', () => {
            setPaste('');
            setDialog('paste');
          })}
        </ScrollView>
        {button('Keyboard', props.onKeyboard)}
      </View>
      <Modal
        visible={dialog !== null}
        transparent
        animationType="none"
        onRequestClose={() => setDialog(null)}
      >
        <KeyboardAvoidingView behavior={Platform.OS === 'ios' ? 'padding' : undefined} style={styles.overlay}>
          <View style={styles.dialog} accessibilityViewIsModal>
            <Text style={styles.heading}>{dialog === 'copy' ? 'Select text to copy' : 'Paste text'}</Text>
            {dialog === 'copy' ? (
              <ScrollView style={styles.editor}>
                <Text selectable style={styles.copy}>
                  {props.selection}
                </Text>
              </ScrollView>
            ) : (
              <TextInput
                autoFocus
                multiline
                value={paste}
                onChangeText={setPaste}
                maxLength={65000}
                style={styles.editor}
                autoCorrect={false}
                autoCapitalize="none"
                placeholder="Touch and hold to paste"
                placeholderTextColor={colors.muted}
                accessibilityLabel="Text to paste into terminal"
              />
            )}
            <View style={styles.actions}>
              {dialog === 'paste' && button('Cancel', () => setDialog(null))}
              {dialog === 'paste' &&
                button(
                  'Send',
                  () => {
                    props.onPaste(paste);
                    setDialog(null);
                  },
                  false,
                  !paste,
                )}
              {dialog === 'copy' && button('Done', () => setDialog(null))}
            </View>
          </View>
        </KeyboardAvoidingView>
      </Modal>
    </>
  );
}
const styles = StyleSheet.create({
  row: {
    flexDirection: 'row',
    gap: 4,
    paddingHorizontal: 4,
    backgroundColor: colors.panel,
    borderTopWidth: 1,
    borderColor: colors.line,
    paddingTop: 4,
  },
  scroll: { flex: 1 },
  keys: { gap: 4 },
  key: {
    backgroundColor: colors.bg,
    borderWidth: 1,
    borderColor: colors.line,
    borderRadius: 3,
    minWidth: 40,
    minHeight: 40,
    paddingHorizontal: 9,
    justifyContent: 'center',
    alignItems: 'center',
  },
  active: { backgroundColor: colors.accent, borderColor: colors.accent },
  disabled: { opacity: 0.4 },
  pressed: { opacity: 0.65 },
  label: { color: colors.text, fontSize: 14 },
  activeLabel: { color: colors.bg },
  overlay: { flex: 1, justifyContent: 'center', backgroundColor: '#0009', padding: 16 },
  dialog: {
    backgroundColor: colors.panel,
    borderWidth: 1,
    borderColor: colors.line,
    padding: 12,
    maxHeight: '80%',
  },
  heading: { color: colors.text, fontSize: 16, marginBottom: 8 },
  editor: {
    color: colors.text,
    backgroundColor: colors.bg,
    minHeight: 100,
    maxHeight: 280,
    borderWidth: 1,
    borderColor: colors.line,
    padding: 8,
    textAlignVertical: 'top',
  },
  copy: { color: colors.text, fontFamily: Platform.OS === 'ios' ? 'Menlo' : 'monospace', fontSize: 13 },
  actions: { flexDirection: 'row', justifyContent: 'flex-end', gap: 8, marginTop: 10 },
});
