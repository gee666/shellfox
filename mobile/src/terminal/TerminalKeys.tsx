import React, { useEffect, useMemo, useRef, useState } from 'react';
import Clipboard from '@react-native-clipboard/clipboard';
import {
  Alert,
  KeyboardAvoidingView,
  Modal,
  Platform,
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
  attach?: React.ReactNode;
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
  const [pasting, setPasting] = useState(false);
  const [paste, setPaste] = useState('');
  const [copied, setCopied] = useState(false);
  const pasteRequest = useRef(0);
  useEffect(() => () => { pasteRequest.current += 1; }, []);
  useEffect(() => setCopied(false), [props.selection]);
  const closePaste = () => {
    pasteRequest.current += 1;
    setPasting(false);
  };
  const openPaste = async () => {
    const request = ++pasteRequest.current;
    setPaste('');
    setPasting(true);
    try {
      const text = await Clipboard.getString();
      if (request === pasteRequest.current) setPaste(text);
    } catch {
      if (request === pasteRequest.current) {
        Alert.alert('Could not paste', 'The clipboard is unavailable. Touch and hold in the text field to paste.');
      }
    }
  };
  const copy = () => {
    if (!props.selection) return;
    try {
      Clipboard.setString(props.selection);
      setCopied(true);
    } catch {
      Alert.alert('Could not copy', 'The clipboard is unavailable. Try again.');
    }
  };
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
          {button('Paste', () => { void openPaste(); })}
        </KeyboardAccessoryScroll>
        {!!props.selection && button(copied ? 'Copied' : 'Copy', copy)}
        {props.attach}
        <AccessoryButton label="Keyboard" onPress={props.onKeyboard} active={props.systemKeyboard}>
          <KeyboardIcon active={props.systemKeyboard} />
        </AccessoryButton>
        <AccessoryButton label="Special keys" onPress={props.onSpecialKeyboard} active={props.specialKeyboard}>
          <KeyboardIcon special active={props.specialKeyboard} />
        </AccessoryButton>
      </KeyboardAccessory>
      <Modal
        visible={pasting}
        transparent
        animationType="none"
        onRequestClose={closePaste}
      >
        <KeyboardAvoidingView behavior={Platform.OS === 'ios' ? 'padding' : undefined} style={styles.overlay}>
          <View style={styles.dialog} accessibilityViewIsModal>
            <Text style={styles.heading}>Paste text</Text>
            <TextInput
              autoFocus
              multiline
              value={paste}
              onChangeText={text => {
                // A delayed clipboard read must not replace the user's edits.
                pasteRequest.current += 1;
                setPaste(text);
              }}
              maxLength={65000}
              style={styles.editor}
              autoCorrect={false}
              autoCapitalize="none"
              placeholder="Touch and hold to paste"
              placeholderTextColor={colors.muted}
              accessibilityLabel="Text to paste into terminal"
            />
            <View style={styles.actions}>
              {button('Cancel', closePaste)}
              {button('Send', () => {
                props.onPaste(paste);
                closePaste();
              }, false, !paste)}
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
  actions: { flexDirection: 'row', justifyContent: 'flex-end', gap: 8, marginTop: 10 },
});
