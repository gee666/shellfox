import React, { useRef } from 'react';
import { Keyboard, Modal, Pressable, TextInput, View } from 'react-native';
import { SafeAreaProvider, useSafeAreaInsets } from 'react-native-safe-area-context';
import type { ManagerApi } from '../../src/shared/contracts';
import { DirectoryPathInput, type DirectoryPathInputHandle } from './DirectoryPathInput';
import { FileIconButton } from './FileIconButton';
import { FILE_MODAL_ORIENTATIONS, FILE_TOOLBAR_HEIGHT } from './FileToolbar';
import { Note, useUi } from './ui';

interface Props {
  api?: Pick<ManagerApi, 'getHomeDirectory' | 'completeDirectory'>;
  value: string;
  change: (value: string) => void;
  submit: (value: string) => void;
  close: () => void;
  editable: boolean;
  hostName?: string;
}
export function FilePathSheet(props: Props) {
  const input = useRef<DirectoryPathInputHandle>(null);
  const dismiss = () => { Keyboard.dismiss(); props.close(); };
  const back = () => { if (!input.current?.dismissSuggestions()) dismiss(); };
  return <Modal transparent visible animationType="none" presentationStyle="overFullScreen" supportedOrientations={FILE_MODAL_ORIENTATIONS} statusBarTranslucent navigationBarTranslucent onRequestClose={back}>
    <SafeAreaProvider><PathCard {...props} input={input} dismiss={dismiss} back={back} /></SafeAreaProvider>
  </Modal>;
}
function PathCard({ api, value, change, submit, editable, hostName, input, dismiss, back }: Props & {
  input: React.RefObject<DirectoryPathInputHandle | null>; dismiss: () => void; back: () => void;
}) {
  const { colors } = useUi();
  const insets = useSafeAreaInsets();
  return <View style={{ flex: 1, paddingLeft: Math.max(6, insets.left + 4), paddingRight: Math.max(6, insets.right + 4) }}>
    <Pressable accessibilityLabel="Dismiss path sheet" onPress={dismiss} style={{ position: 'absolute', top: 0, bottom: 0, left: 0, right: 0, backgroundColor: colors.backdrop }} />
    <View testID="files-path-sheet" collapsable={false} accessibilityViewIsModal
      onAccessibilityEscape={back} style={{ marginTop: insets.top + FILE_TOOLBAR_HEIGHT + 4, padding: 6, width: '100%',
        maxWidth: 520, alignSelf: 'flex-end', backgroundColor: colors.panel, borderColor: colors.line, borderWidth: 1, borderRadius: 3 }}>
      {hostName && <View style={{ paddingHorizontal: 36, paddingBottom: 4 }}><Note>{hostName}</Note></View>}
      <View style={{ position: 'relative', paddingHorizontal: 36 }}>
        {api ? <DirectoryPathInput ref={input} api={api} value={value} onChangeText={change} onSubmit={submit} onClose={dismiss}
          label="File path" compact autoFocus editable={editable} />
          : <TextInput accessibilityLabel="File path" value={value} onChangeText={change} editable={editable} autoFocus
            autoCapitalize="none" autoCorrect={false} disableFullscreenUI returnKeyType="go" onSubmitEditing={() => submit(value)}
            style={{ height: 36, paddingHorizontal: 8, paddingVertical: 6, color: colors.text, borderColor: colors.line, borderWidth: 1, borderRadius: 3, fontSize: 14 }} />}
        <View style={{ position: 'absolute', left: 0, top: 2 }}><FileIconButton testID="files-path-close" icon="close" label="Close path" onPress={dismiss} /></View>
        <View style={{ position: 'absolute', right: 0, top: 2 }}><FileIconButton testID="files-path-go" icon="go" label="Go" onPress={() => submit(value)} disabled={!editable} /></View>
      </View>
    </View>
  </View>;
}
