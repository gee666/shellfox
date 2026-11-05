import React, { useState } from 'react';
import { Pressable, Text, View } from 'react-native';
import { useUi } from './ui';

type FileIcon = 'preview' | 'edit' | 'reload';
/** Native border geometry, matching the app's keyboard icons without a font or SVG dependency. */
function FileIconGlyph({ icon, color }: { icon: FileIcon; color: string }) {
  if (icon === 'preview') return <View accessible={false} style={{ width: 24, height: 16, borderWidth: 1.7, borderColor: color, borderRadius: 12, alignItems: 'center', justifyContent: 'center' }}>
    <View style={{ width: 7, height: 7, borderWidth: 1.7, borderColor: color, borderRadius: 4 }} />
  </View>;
  if (icon === 'edit') return <View accessible={false} style={{ width: 22, height: 22 }}>
    <View style={{ position: 'absolute', left: 0, bottom: 0, width: 18, height: 17, borderWidth: 1.7, borderColor: color, borderRadius: 2 }} />
    <View style={{ position: 'absolute', right: 1, top: -2, width: 5, height: 20, borderWidth: 1.7, borderColor: color, backgroundColor: 'transparent', transform: [{ rotate: '40deg' }] }} />
  </View>;
  return <View accessible={false} style={{ width: 22, height: 22, borderWidth: 1.7, borderColor: color, borderLeftColor: 'transparent', borderRadius: 12, transform: [{ rotate: '-30deg' }] }}>
    <View style={{ position: 'absolute', left: -2, top: -1, width: 7, height: 7, borderLeftWidth: 1.7, borderBottomWidth: 1.7, borderColor: color, transform: [{ rotate: '-20deg' }] }} />
  </View>;
}
export function FileIconButton({ icon, label, onPress, disabled = false, active = false, busy = false, testID }: {
  icon: FileIcon; label: string; onPress: () => void; disabled?: boolean; active?: boolean; busy?: boolean; testID?: string;
}) {
  const { colors } = useUi();
  const [tooltip, setTooltip] = useState(false);
  return <View style={{ position: 'relative' }}>
    <Pressable accessibilityRole="button" accessibilityLabel={label} accessibilityHint="Hold to show the button name"
      accessibilityState={{ disabled, selected: active, busy }} testID={testID} disabled={disabled} onPress={onPress}
      onLongPress={() => setTooltip(true)} onPressOut={() => setTooltip(false)} onFocus={() => setTooltip(true)} onBlur={() => setTooltip(false)}
      style={({ pressed }) => ({ width: 44, height: 44, borderRadius: 3, alignItems: 'center', justifyContent: 'center',
        backgroundColor: active ? colors.accent : colors.surface, opacity: disabled ? 0.4 : pressed ? 0.65 : 1 })}>
      <FileIconGlyph icon={icon} color={active ? colors.onAccent : colors.text} />
    </Pressable>
    {tooltip && <Text pointerEvents="none" style={{ position: 'absolute', top: 46, right: 0, zIndex: 10, padding: 5,
      minWidth: 90, color: colors.text, backgroundColor: colors.panel, borderRadius: 2, fontSize: 12 }}>{label}</Text>}
  </View>;
}
