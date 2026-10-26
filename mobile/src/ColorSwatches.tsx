import React, { useState } from 'react';
import { Pressable, Text, View } from 'react-native';
import { colorSchema, customColors, presetsFor, saveCustomColor, type ColorKind } from './color-settings';
import { ColorPicker } from './ColorPicker';
import { useUi } from './ui';

export function ColorSwatches({ kind, value, saved, onChange, onSave, disabled = false }: {
  kind: ColorKind;
  value: string;
  saved: readonly string[];
  onChange: (color: string) => void;
  onSave: (colors: string[]) => void;
  disabled?: boolean;
}) {
  const { colors, styles } = useUi();
  const [picking, setPicking] = useState(false);
  const presets = presetsFor(kind);
  const custom = customColors(kind, saved);
  const color = value.toLowerCase();
  const canAdd = colorSchema.safeParse(color).success && !presets.includes(color) && !custom.includes(color);
  const label = kind === 'accent' ? 'Accent color' : 'Background color';
  const swatch = (item: string, savedColor = false) => (
    <Pressable
      key={item}
      accessibilityRole="button"
      accessibilityLabel={savedColor ? `Use custom ${kind} ${item}` : kind === 'accent' ? `Use ${item}` : `Use background ${item}`}
      accessibilityState={{ selected: color === item, disabled }}
      disabled={disabled}
      onPress={() => onChange(item)}
      style={{ width: 36, height: 40, justifyContent: 'center', alignItems: 'center', opacity: disabled ? 0.4 : 1 }}
    >
      <View style={{ width: 28, height: 28, padding: 3, borderRadius: 3, borderWidth: 2, borderColor: color === item ? colors.text : colors.line }}>
        <View style={{ flex: 1, backgroundColor: item }} />
      </View>
    </Pressable>
  );
  return (
    <View style={{ gap: 4 }}>
      <Text style={styles.label}>{label}</Text>
      <View style={{ flexDirection: 'row', flexWrap: 'wrap', alignItems: 'center' }}>
        {presets.map(item => swatch(item))}
        {custom.length > 0 && (
          <View accessibilityLabel={`Custom ${kind} colors`} style={{ flexDirection: 'row', marginLeft: 4, paddingLeft: 4, borderLeftWidth: 1, borderColor: colors.line }}>
            {custom.map(item => swatch(item, true))}
          </View>
        )}
        <Pressable
          accessibilityRole="button"
          accessibilityLabel={label}
          accessibilityState={{ disabled }}
          disabled={disabled}
          onPress={() => setPicking(true)}
          style={{ width: 40, height: 40, justifyContent: 'center', alignItems: 'center', opacity: disabled ? 0.4 : 1 }}
        >
          <View style={{ width: 28, height: 28, borderWidth: 1, borderColor: colors.text, backgroundColor: color, alignItems: 'center', justifyContent: 'center' }}>
            <Text style={{ color: colors.text, backgroundColor: colors.panel, fontSize: 12 }}>…</Text>
          </View>
        </Pressable>
        <Pressable
          accessibilityRole="button"
          accessibilityLabel={`Save custom ${kind} color`}
          accessibilityHint="Keeps the last 3."
          accessibilityState={{ disabled: disabled || !canAdd }}
          disabled={disabled || !canAdd}
          onPress={() => onSave(saveCustomColor(kind, custom, color))}
          style={{ width: 40, height: 40, justifyContent: 'center', alignItems: 'center', opacity: disabled || !canAdd ? 0.35 : 1 }}
        >
          <Text style={{ color: colors.text, fontSize: 22 }}>+</Text>
        </Pressable>
      </View>
      {picking && <ColorPicker label={label} value={value} onChange={onChange} onClose={() => setPicking(false)} />}
    </View>
  );
}
