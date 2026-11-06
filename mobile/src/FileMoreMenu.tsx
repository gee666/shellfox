import React from 'react';
import { Modal, Pressable, ScrollView, Text, View, useWindowDimensions } from 'react-native';
import { SafeAreaProvider, useSafeAreaInsets } from 'react-native-safe-area-context';
import { FILE_MODAL_ORIENTATIONS, FILE_TOOLBAR_HEIGHT } from './FileToolbar';
import { useUi } from './ui';

export interface FileMenuItem { label: string; action: () => void; disabled?: boolean }
interface Props { path: string; hostName?: string; items: FileMenuItem[]; close: () => void }
export function FileMoreMenu(props: Props) {
  return <Modal transparent visible animationType="none" presentationStyle="overFullScreen" supportedOrientations={FILE_MODAL_ORIENTATIONS} statusBarTranslucent navigationBarTranslucent onRequestClose={props.close}>
    <SafeAreaProvider><MenuCard {...props} /></SafeAreaProvider>
  </Modal>;
}
function MenuCard({ path, hostName, items, close }: Props) {
  const { colors } = useUi();
  const insets = useSafeAreaInsets();
  const { height } = useWindowDimensions();
  return <View style={{ flex: 1, paddingLeft: Math.max(6, insets.left + 4), paddingRight: Math.max(6, insets.right + 4) }}>
    <Pressable accessibilityLabel="Close menu" onPress={close} style={{ position: 'absolute', top: 0, bottom: 0, left: 0, right: 0, backgroundColor: colors.backdrop }} />
    <View testID="files-menu" collapsable={false} accessibilityViewIsModal onAccessibilityEscape={close}
      style={{ marginTop: insets.top + FILE_TOOLBAR_HEIGHT + 4, alignSelf: 'flex-end', width: 280, maxWidth: '100%',
        maxHeight: Math.max(100, height - insets.top - insets.bottom - FILE_TOOLBAR_HEIGHT - 16),
        padding: 8, backgroundColor: colors.panel, borderColor: colors.line, borderWidth: 1, borderRadius: 3 }}>
      <ScrollView style={{ maxHeight: 84, flexGrow: 0 }} nestedScrollEnabled>
        {hostName && <Text style={{ color: colors.muted, fontSize: 12, marginBottom: 3 }}>{hostName}</Text>}
        <Text selectable accessibilityLabel={path} style={{ color: colors.text, fontSize: 13 }}>{path}</Text>
      </ScrollView>
      <ScrollView style={{ flexShrink: 1 }} keyboardShouldPersistTaps="handled">
        {items.map(item => <Pressable key={item.label} accessibilityRole="button" accessibilityLabel={item.label} disabled={item.disabled}
          accessibilityState={{ disabled: !!item.disabled }} onPress={() => { close(); item.action(); }}
          style={({ pressed }) => ({ minHeight: 38, justifyContent: 'center', paddingHorizontal: 6,
            opacity: item.disabled ? 0.35 : 1, backgroundColor: pressed ? colors.surface : 'transparent' })}>
          <Text style={{ color: colors.text, fontSize: 14 }}>{item.label}</Text>
        </Pressable>)}
      </ScrollView>
    </View>
  </View>;
}
