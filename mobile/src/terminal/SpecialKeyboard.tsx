import React from 'react';
import { View } from 'react-native';
import { AccessoryButton, KeyboardAccessory, KeyboardAccessoryScroll } from './KeyboardAccessory';
import type { TerminalKey, TerminalModifiers } from './protocol';

interface Props {
  modifiers: TerminalModifiers;
  onModifier: (modifier: keyof TerminalModifiers) => void;
  onKey: (key: TerminalKey) => void;
  bottomInset: number;
}
const functions = ['F1', 'F2', 'F3', 'F4', 'F5', 'F6', 'F7', 'F8', 'F9', 'F10', 'F11', 'F12'];
const navigation = [
  ['Home', 'Home'], ['End', 'End'], ['PgUp', 'PageUp'], ['PgDn', 'PageDown'],
  ['Insert', 'Insert'], ['Delete', 'Delete'],
] as const;
const shortcuts = ['c', 'd', 'j', 'z', 'l', '[', ']', '\\', '/', '|', '~'];

export function SpecialKeyboard({ modifiers, onModifier, onKey, bottomInset }: Props) {
  return <View accessibilityLabel="Special keys keyboard">
    <KeyboardAccessory>
      <KeyboardAccessoryScroll>
        {([['Alt', 'alt'], ['Shift', 'shift'], ['Super', 'meta'], ['CapsLock', 'capsLock']] as const)
          .map(([label, modifier]) => <AccessoryButton key={label} label={label}
            active={modifiers[modifier]} onPress={() => onModifier(modifier)} />)}
        <AccessoryButton label="Enter" onPress={() => onKey('Enter')} />
        <AccessoryButton label="Backspace" onPress={() => onKey('Backspace')} />
      </KeyboardAccessoryScroll>
    </KeyboardAccessory>
    <KeyboardAccessory><KeyboardAccessoryScroll>
      {functions.map(key => <AccessoryButton key={key} label={key} onPress={() => onKey(key)} />)}
    </KeyboardAccessoryScroll></KeyboardAccessory>
    <KeyboardAccessory><KeyboardAccessoryScroll>
      {navigation.map(([label, key]) => <AccessoryButton key={key} label={label} onPress={() => onKey(key)} />)}
    </KeyboardAccessoryScroll></KeyboardAccessory>
    <KeyboardAccessory bottomInset={bottomInset}><KeyboardAccessoryScroll>
      {shortcuts.map(key => <AccessoryButton key={key} label={key} onPress={() => onKey(key)} />)}
    </KeyboardAccessoryScroll></KeyboardAccessory>
  </View>;
}
