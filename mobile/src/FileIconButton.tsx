import React from 'react';
import { ActivityIndicator, Pressable, View } from 'react-native';
import { useUi } from './ui';

export type FileIcon = 'preview' | 'edit' | 'reload' | 'close' | 'back' | 'folder' | 'copy' | 'download' | 'upload' | 'save' | 'wrap' | 'more' | 'code' | 'go' | 'document';
/** App-owned geometry. Every glyph uses the same centered 20dp box. */
export function FileIconGlyph({ icon, color, testID }: { icon: FileIcon; color: string; testID?: string }) {
  const line = { position: 'absolute' as const, backgroundColor: color, borderRadius: 1 };
  const border = { position: 'absolute' as const, borderColor: color, borderWidth: 1.5, borderRadius: 1 };
  return <View testID={testID} collapsable={false} accessible={false} pointerEvents="none" style={{ width: 20, height: 20, alignItems: 'center', justifyContent: 'center' }}>
    {icon === 'close' ? <>
      <View style={[line, { left: 9.25, top: 2, width: 1.5, height: 16, transform: [{ rotate: '45deg' }] }]} />
      <View style={[line, { left: 9.25, top: 2, width: 1.5, height: 16, transform: [{ rotate: '-45deg' }] }]} />
    </> : icon === 'back' || icon === 'go' ? <View style={{ width: 10, height: 10, marginLeft: icon === 'back' ? 7 : -7, borderLeftWidth: 1.5, borderBottomWidth: 1.5, borderColor: color, transform: [{ rotate: icon === 'back' ? '45deg' : '225deg' }] }} />
      : icon === 'folder' ? <>
        <View style={[border, { left: 1, top: 3, width: 8, height: 5, borderBottomWidth: 0 }]} />
        <View style={[border, { left: 1, top: 7, width: 18, height: 11 }]} />
      </> : icon === 'document' ? <>
        <View style={[border, { left: 3, top: 1, width: 14, height: 18 }]} />
        <View style={[line, { left: 6, top: 7, width: 8, height: 1.5 }]} />
        <View style={[line, { left: 6, top: 11, width: 8, height: 1.5 }]} />
      </> : icon === 'copy' ? <>
        <View style={[border, { left: 2, top: 1, width: 12, height: 14 }]} />
        <View style={[border, { left: 6, top: 5, width: 12, height: 14 }]} />
      </> : icon === 'download' || icon === 'upload' ? <>
        <View style={[line, { left: 9.25, top: icon === 'download' ? 1 : 3, width: 1.5, height: 10 }]} />
        <View style={{ position: 'absolute', left: 6.5, top: icon === 'download' ? 4.5 : 2, width: 7, height: 7,
          borderColor: color, borderRightWidth: icon === 'download' ? 1.5 : 0, borderBottomWidth: icon === 'download' ? 1.5 : 0,
          borderLeftWidth: icon === 'upload' ? 1.5 : 0, borderTopWidth: icon === 'upload' ? 1.5 : 0, transform: [{ rotate: '45deg' }] }} />
        <View style={{ position: 'absolute', left: 3, bottom: 1, width: 14, height: 5, borderLeftWidth: 1.5, borderRightWidth: 1.5, borderBottomWidth: 1.5, borderColor: color }} />
      </> : icon === 'save' ? <>
        <View style={[border, { left: 2, top: 2, width: 16, height: 16 }]} />
        <View style={[border, { left: 6, top: 2, width: 8, height: 6, borderTopWidth: 0 }]} />
        <View style={[border, { left: 5, top: 11, width: 10, height: 7, borderBottomWidth: 0 }]} />
      </> : icon === 'more' ? [3, 8.5, 14].map(left => <View key={left} style={[line, { left, top: 8.5, width: 3, height: 3, borderRadius: 2 }]} />)
      : icon === 'preview' ? <View style={{ width: 19, height: 12, borderWidth: 1.5, borderColor: color, borderRadius: 10, alignItems: 'center', justifyContent: 'center' }}>
        <View style={{ width: 5, height: 5, borderWidth: 1.5, borderColor: color, borderRadius: 3 }} />
      </View> : icon === 'edit' ? <>
        <View style={[border, { left: 2, bottom: 2, width: 13, height: 12 }]} />
        <View style={[border, { right: 4, top: 1, width: 4, height: 15, transform: [{ rotate: '40deg' }] }]} />
      </> : icon === 'code' ? <>
        <View style={{ position: 'absolute', left: 3, top: 6, width: 7, height: 7, borderLeftWidth: 1.5, borderBottomWidth: 1.5, borderColor: color, transform: [{ rotate: '45deg' }] }} />
        <View style={{ position: 'absolute', right: 3, top: 6, width: 7, height: 7, borderRightWidth: 1.5, borderTopWidth: 1.5, borderColor: color, transform: [{ rotate: '45deg' }] }} />
      </> : icon === 'wrap' ? <>
        {[4, 9, 15].map((top, index) => <View key={top} style={[line, { left: 2, top, width: index === 1 ? 12 : 16, height: 1.5 }]} />)}
        <View style={{ position: 'absolute', right: 2, top: 8, width: 5, height: 5, borderRightWidth: 1.5, borderBottomWidth: 1.5, borderColor: color, borderBottomRightRadius: 3 }} />
      </> : <View style={{ width: 16, height: 16, borderWidth: 1.5, borderColor: color, borderLeftColor: 'transparent', borderRadius: 9, transform: [{ rotate: '-30deg' }] }}>
        <View style={{ position: 'absolute', left: -2, top: -1, width: 5, height: 5, borderLeftWidth: 1.5, borderBottomWidth: 1.5, borderColor: color, transform: [{ rotate: '-20deg' }] }} />
      </View>}
  </View>;
}
export function FileIconButton({ icon, label, onPress, disabled = false, active = false, busy = false, testID, size = 32 }: {
  icon: FileIcon; label: string; onPress: () => void; disabled?: boolean; active?: boolean; busy?: boolean; testID?: string; size?: 32 | 36;
}) {
  const { colors } = useUi();
  return <Pressable accessibilityRole="button" accessibilityLabel={label}
    accessibilityState={{ disabled, selected: active, busy }} testID={testID} disabled={disabled} onPress={onPress}
    hitSlop={1} style={({ pressed }) => ({ width: size, height: size, flexShrink: 0, borderRadius: 3, alignItems: 'center', justifyContent: 'center',
      backgroundColor: active || pressed ? colors.surface : 'transparent', opacity: disabled ? 0.35 : pressed ? 0.65 : 1 })}>
    {busy ? <View testID={testID ? `${testID}-glyph` : undefined} style={{ width: 20, height: 20, alignItems: 'center', justifyContent: 'center' }}><ActivityIndicator size="small" color={colors.accent} /></View>
      : <FileIconGlyph icon={icon} color={active ? colors.accent : colors.text} testID={testID ? `${testID}-glyph` : undefined} />}
  </Pressable>;
}
