import React, { useMemo, useState } from 'react';
import {
  KeyboardAvoidingView,
  Modal,
  Platform,
  ScrollView,
  StyleSheet,
  Text,
  TextInput,
  View,
} from 'react-native';
import { useTheme, type ThemeColors } from '../theme';
import { AccessoryButton, KeyboardAccessory, KeyboardAccessoryScroll } from './KeyboardAccessory';
import { KeyboardIcon } from './KeyboardIcon';
import type { TerminalKey } from './protocol';

interface Props {
  bottomInset: number;
  control: boolean;
  selecting: boolean;
  selection: string;
  onControl: () => void;
  onSelect: () => void;
  onKey: (key: TerminalKey) => void;
  systemKeyboard: boolean;
  specialKeyboard: boolean;
  onKeyboard: () => void;
  onSpecialKeyboard: () => void;
  onPaste: (text: string) => void;
}
const keys = [
  ['Esc', 'Escape'],
  ['Tab', 'Tab'],
  ['↑', 'ArrowUp'],
  ['↓', 'ArrowDown'],
  ['←', 'ArrowLeft'],
  ['→', 'ArrowRight'],
] as const;
export function TerminalKeys(props: Props) {
  const { colors } = useTheme();
  const styles = useMemo(() => createStyles(colors), [colors]);
  const [dialog, setDialog] = useState<'copy' | 'paste' | null>(null);
  const [paste, setPaste] = useState('');
  const button = (label: string, onPress: () => void, active = false, disabled = false) => (
    <AccessoryButton key={label} label={label} onPress={onPress} active={active} disabled={disabled} />
  );
  return (
    <>
      <KeyboardAccessory bottomInset={props.bottomInset}>
        <KeyboardAccessoryScroll>
          {button('Ctrl', props.onControl, props.control)}
          {keys.map(([label, data]) => button(label, () => props.onKey(data)))}
          {button('Select', props.onSelect, props.selecting)}
          {button('Copy', () => setDialog('copy'), false, !props.selection)}
          {button('Paste', () => {
            setPaste('');
            setDialog('paste');
          })}
        </KeyboardAccessoryScroll>
        <AccessoryButton label="Keyboard" onPress={props.onKeyboard} active={props.systemKeyboard}>
          <KeyboardIcon active={props.systemKeyboard} />
        </AccessoryButton>
        <AccessoryButton label="Special keys" onPress={props.onSpecialKeyboard} active={props.specialKeyboard}>
          <KeyboardIcon special active={props.specialKeyboard} />
        </AccessoryButton>
      </KeyboardAccessory>
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
const createStyles = (colors: ThemeColors) => StyleSheet.create({
  overlay: { flex: 1, justifyContent: 'center', backgroundColor: colors.backdrop, padding: 16 },
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
