import React from 'react';
import { StyleSheet, Text, View } from 'react-native';
import { useTheme } from '../theme';

/** Small vector-like keyboard built from native Views, with no platform font dependency. */
export function KeyboardIcon({ special = false, active = false }: { special?: boolean; active?: boolean }) {
  const { colors } = useTheme();
  const color = active ? colors.onAccent : colors.text;
  if (special) return <View accessible={false} style={[styles.specialKey, { borderColor: color }]}>
    <Text style={[styles.fn, { color }]}>Fn</Text>
  </View>;
  return <View accessible={false} style={[styles.frame, { borderColor: color }]}>
    {[0, 1].map(row => <View key={row} style={styles.row}>
      {[0, 1, 2, 3].map(col => <View key={col} style={[styles.dot, { backgroundColor: color }]} />)}
    </View>)}
    <View style={[styles.space, { backgroundColor: color }]} />
  </View>;
}
const styles = StyleSheet.create({
  frame: { width: 24, height: 18, borderWidth: 1.5, borderRadius: 2, padding: 3, gap: 2 },
  row: { flexDirection: 'row', justifyContent: 'space-between' },
  dot: { width: 2, height: 2 },
  space: { width: 10, height: 2, alignSelf: 'center' },
  specialKey: { width: 24, height: 22, borderWidth: 1.5, borderRadius: 3,
    alignItems: 'center', justifyContent: 'center' },
  fn: { fontSize: 12, fontWeight: '600' },
});
