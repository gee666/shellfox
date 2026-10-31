import React, { useRef, useState } from 'react';
import { Text, View, type GestureResponderEvent } from 'react-native';
import {
  MIN_MODIFIER_RELEASE_DELAY_MS as MIN,
  MAX_MODIFIER_RELEASE_DELAY_MS as MAX,
  MODIFIER_RELEASE_DELAY_STEP_MS as STEP,
} from './preferences';
import { useUi } from './ui';

const THUMB_SIZE = 24;
const snap = (value: number) => Math.max(MIN, Math.min(MAX, MIN + Math.round((value - MIN) / STEP) * STEP));

export function ModifierReleaseDelaySlider({ value, disabled = false, onChange }: {
  value: number;
  disabled?: boolean;
  onChange: (value: number) => void;
}) {
  const { colors, styles } = useUi();
  const width = useRef(0);
  const dragging = useRef(false);
  const [draft, setDraft] = useState<number | null>(null);
  const shown = draft ?? value;
  const fraction = (shown - MIN) / (MAX - MIN);
  const choose = (event: GestureResponderEvent): number | null => {
    if (disabled || width.current <= THUMB_SIZE) return null;
    return snap(MIN + (event.nativeEvent.locationX - THUMB_SIZE / 2) / (width.current - THUMB_SIZE) * (MAX - MIN));
  };
  const preview = (event: GestureResponderEvent) => {
    const next = choose(event);
    if (next !== null) setDraft(next);
  };
  const commit = (next: number) => {
    if (!disabled && next !== value) onChange(next);
  };
  return (
    <View>
      <View style={styles.row}>
        <Text style={[styles.label, styles.grow]}>Sticky modifier release delay</Text>
        <Text style={styles.note}>{shown} ms</Text>
      </View>
      <View
        accessible
        accessibilityRole="adjustable"
        accessibilityLabel="Sticky modifier release delay"
        accessibilityHint="Delay before sticky modifiers release after typing."
        accessibilityValue={{ min: MIN, max: MAX, now: shown, text: `${shown} milliseconds` }}
        accessibilityState={{ disabled }}
        accessibilityActions={[{ name: 'increment', label: 'Longer' }, { name: 'decrement', label: 'Shorter' }]}
        onAccessibilityAction={event => {
          if (event.nativeEvent.actionName === 'increment') commit(snap(value + STEP));
          if (event.nativeEvent.actionName === 'decrement') commit(snap(value - STEP));
        }}
        onLayout={event => { width.current = event.nativeEvent.layout.width; }}
        onStartShouldSetResponder={() => !disabled && width.current > THUMB_SIZE}
        onMoveShouldSetResponder={() => !disabled && width.current > THUMB_SIZE}
        onResponderGrant={event => { dragging.current = !disabled; preview(event); }}
        onResponderMove={event => { if (dragging.current) preview(event); }}
        onResponderRelease={event => {
          const next = dragging.current ? choose(event) : null;
          dragging.current = false;
          setDraft(null);
          if (next !== null) commit(next);
        }}
        onResponderTerminationRequest={() => false}
        onResponderTerminate={() => { dragging.current = false; setDraft(null); }}
        style={{ height: 44, justifyContent: 'center', opacity: disabled ? 0.4 : 1 }}
      >
        <View pointerEvents="none" style={{ marginHorizontal: THUMB_SIZE / 2, height: 4, backgroundColor: colors.line }}>
          <View style={{ width: `${fraction * 100}%`, height: 4, backgroundColor: colors.accent }} />
          <View style={{
            position: 'absolute', left: `${fraction * 100}%`, top: (4 - THUMB_SIZE) / 2,
            marginLeft: -THUMB_SIZE / 2, width: THUMB_SIZE, height: THUMB_SIZE,
            borderRadius: THUMB_SIZE / 2, backgroundColor: colors.accent,
          }} />
        </View>
      </View>
      <View style={[styles.row, { justifyContent: 'space-between' }]}>
        <Text style={styles.note}>Shorter</Text>
        <Text style={styles.note}>Longer</Text>
      </View>
    </View>
  );
}
