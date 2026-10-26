import React, { useState } from 'react';
import { Modal, Pressable, Text, View, type GestureResponderEvent } from 'react-native';
import { colorSchema } from './color-settings';
import { hexToHsv, hsvToHex } from './color-picker';
import { Button, Field, useUi } from './ui';

const STEPS = Array.from({ length: 16 }, (_, index) => index / 15);
const HUES = Array.from({ length: 24 }, (_, index) => index * 15);

export function ColorPicker({ label, value, onChange, onClose }: {
  label: string;
  value: string;
  onChange: (color: string) => void;
  onClose: () => void;
}) {
  const { colors, styles } = useUi();
  const [draft, setDraft] = useState(value);
  const [hsv, setHsv] = useState(() => hexToHsv(value));
  const [size, setSize] = useState({ width: 1, height: 1 });
  const parsed = colorSchema.safeParse(draft);
  const choose = (event: GestureResponderEvent) => {
    const next = {
      hue: hsv.hue,
      saturation: Math.max(0, Math.min(1, event.nativeEvent.locationX / size.width)),
      value: 1 - Math.max(0, Math.min(1, event.nativeEvent.locationY / size.height)),
    };
    setHsv(next);
    setDraft(hsvToHex(next));
  };
  return (
    <Modal transparent animationType="none" onRequestClose={onClose}>
      <View style={{ flex: 1, backgroundColor: colors.backdrop, justifyContent: 'center', padding: 20 }}>
        <View accessibilityViewIsModal style={styles.card}>
          <Text style={styles.label}>{label}</Text>
          <View
            accessibilityLabel={`${label} saturation and brightness`}
            onLayout={event => setSize(event.nativeEvent.layout)}
            onStartShouldSetResponder={() => true}
            onMoveShouldSetResponder={() => true}
            onResponderGrant={choose}
            onResponderMove={choose}
            style={{ height: 160 }}
          >
            <View pointerEvents="none" style={{ flex: 1 }}>
              {STEPS.map(brightness => (
                <View key={brightness} style={{ flex: 1, flexDirection: 'row' }}>
                  {STEPS.map(saturation => (
                    <View key={saturation} style={{ flex: 1, backgroundColor: hsvToHex({ hue: hsv.hue, saturation, value: 1 - brightness }) }} />
                  ))}
                </View>
              ))}
            </View>
            <View pointerEvents="none" style={{ position: 'absolute', left: `${hsv.saturation * 100}%`, top: `${(1 - hsv.value) * 100}%`, width: 10, height: 10, marginLeft: -5, marginTop: -5, borderRadius: 5, borderWidth: 2, borderColor: '#ffffff', backgroundColor: '#000000' }} />
          </View>
          <View style={{ flexDirection: 'row' }}>
            {HUES.map(hue => (
              <Pressable
                key={hue}
                accessibilityRole="button"
                accessibilityLabel={`${label} hue ${hue}`}
                accessibilityState={{ selected: Math.abs(hsv.hue - hue) < 15 }}
                style={{ flex: 1, height: 32, backgroundColor: hsvToHex({ hue, saturation: 1, value: 1 }) }}
                onPress={() => {
                  const next = { ...hsv, hue };
                  setHsv(next);
                  setDraft(hsvToHex(next));
                }}
              />
            ))}
          </View>
          <View style={styles.row}>
            <View style={{ width: 40, height: 40, borderWidth: 1, borderColor: colors.line, backgroundColor: parsed.success ? parsed.data : value }} />
            <View style={styles.grow}>
              <Field label="Hex color" value={draft} maxLength={7} autoCapitalize="none" autoCorrect={false} onChangeText={text => {
                setDraft(text);
                const color = colorSchema.safeParse(text);
                if (color.success) setHsv(hexToHsv(color.data));
              }} />
            </View>
          </View>
          <View style={[styles.row, { justifyContent: 'flex-end' }]}>
            <Button title="Cancel" subtle onPress={onClose} />
            <Button title="Apply" disabled={!parsed.success} onPress={() => {
              if (parsed.success) { onChange(parsed.data); onClose(); }
            }} />
          </View>
        </View>
      </View>
    </Modal>
  );
}
