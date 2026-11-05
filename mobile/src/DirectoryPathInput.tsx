import React, { forwardRef, useEffect, useImperativeHandle, useRef, useState, type ComponentRef, type ReactNode } from 'react';
import { Text, TextInput, View } from 'react-native';
import type { ManagerApi } from '../../src/shared/contracts';
import { visibleDirectoryCompletions } from '../../src/shared/directory-completion';
import { AccessoryButton, KeyboardAccessory, KeyboardAccessoryScroll } from './terminal/KeyboardAccessory';
import { useUi } from './ui';

export interface DirectoryPathInputHandle {
  dismissSuggestions(): boolean;
  focus(): void;
}
export interface DirectoryPathInputProps {
  api: Pick<ManagerApi, 'getHomeDirectory' | 'completeDirectory'>;
  value: string;
  onChangeText(value: string): void;
  onSubmit(path: string): void;
  onClose?: () => void;
  label?: string;
  editable?: boolean;
  autoFocus?: boolean;
  bottomInset?: number;
  homeDirectory?: string;
  renderSuggestions?: (suggestions: ReactNode) => ReactNode;
}
function completionName(match: string): string {
  const separator = match.at(-1);
  if (separator !== '/' && separator !== '\\') return match;
  const path = match.slice(0, -1);
  return path.slice(path.lastIndexOf(separator) + 1);
}
function composing(event: object): boolean {
  return 'isComposing' in event && event.isComposing === true || 'keyCode' in event && event.keyCode === 229;
}

export const DirectoryPathInput = forwardRef<DirectoryPathInputHandle, DirectoryPathInputProps>(function DirectoryPathInput({
  api, value, onChangeText, onSubmit, onClose, label = 'Host directory', editable = true, autoFocus = false,
  bottomInset = 0, homeDirectory, renderSuggestions,
}, ref) {
  const { colors, styles } = useUi();
  const input = useRef<ComponentRef<typeof TextInput>>(null);
  const version = useRef(0);
  const matchesRef = useRef<readonly string[]>([]);
  const dismissed = useRef(false);
  const [matches, setMatches] = useState<readonly string[]>([]);
  const [reopened, setReopened] = useState(0);
  const [home, setHome] = useState('~');
  const updateMatches = (next: readonly string[]) => { matchesRef.current = next; setMatches(next); };
  const clear = () => { version.current++; updateMatches([]); };
  const dismissSuggestions = () => {
    const open = matchesRef.current.length > 0;
    dismissed.current = true;
    clear();
    return open;
  };
  const reopen = () => {
    if (!editable) return;
    dismissed.current = false;
    setReopened(count => count + 1);
  };
  useImperativeHandle(ref, () => ({ dismissSuggestions, focus: () => input.current?.focus() }));
  useEffect(() => {
    let active = true;
    setHome(homeDirectory ?? '~');
    if (homeDirectory === undefined) {
      void api.getHomeDirectory().then(result => {
        if (active && result.ok) setHome(result.value.cwd);
      }).catch(() => {});
    }
    return () => { active = false; };
  }, [api, homeDirectory]);
  useEffect(() => {
    const ticket = ++version.current;
    updateMatches([]);
    if (!editable || dismissed.current) return;
    const timer = setTimeout(() => {
      void api.completeDirectory({ path: value }).then(result => {
        if (ticket === version.current && !dismissed.current && result.ok) {
          updateMatches(visibleDirectoryCompletions(value, result.value.matches));
        }
      }).catch(() => {});
    }, 100);
    return () => { clearTimeout(timer); version.current++; };
  }, [api, value, editable, reopened]);
  const change = (path: string) => {
    clear();
    dismissed.current = false;
    onChangeText(path);
  };
  const choose = (path: string) => { change(path); input.current?.focus(); };
  const submit = () => {
    if (!editable) return;
    const first = matchesRef.current[0];
    if (first !== undefined) { choose(first); return; }
    dismissSuggestions();
    onSubmit(value);
  };
  const suggestions = matches.length > 0 && editable ? (
    <KeyboardAccessory bottomInset={bottomInset}>
      <KeyboardAccessoryScroll>
        {matches.map(match => (
          <AccessoryButton key={match} label={match} onPress={() => choose(match)}>
            <Text style={{ color: colors.text, fontSize: 14 }}>{completionName(match)}</Text>
          </AccessoryButton>
        ))}
      </KeyboardAccessoryScroll>
    </KeyboardAccessory>
  ) : null;
  return <View style={styles.field}>
    <Text style={styles.label}>{label}</Text>
    <View style={[styles.input, { flexDirection: 'row', alignItems: 'center', paddingVertical: 0 }]}>
      <Text accessibilityLabel={`Home directory: ${home}`} style={{ color: colors.muted }}>~ › </Text>
      <TextInput ref={input} accessibilityLabel={label} value={value} onChangeText={change}
        editable={editable} autoFocus={autoFocus} autoCorrect={false} autoCapitalize="none"
        placeholder="folder path" placeholderTextColor={colors.placeholder}
        style={{ flex: 1, minHeight: 44, paddingVertical: 10, color: colors.text, fontSize: 15 }}
        returnKeyType="done" submitBehavior="submit"
        onFocus={reopen} onPressIn={reopen}
        onKeyPress={event => {
          if (composing(event.nativeEvent)) { event.stopPropagation(); return; }
          if (event.nativeEvent.key === 'Escape') {
            event.stopPropagation();
            if (!dismissSuggestions()) onClose?.();
          }
        }}
        onSubmitEditing={event => { if (!composing(event.nativeEvent)) submit(); }} />
    </View>
    {renderSuggestions ? renderSuggestions(suggestions) : suggestions}
  </View>;
});
