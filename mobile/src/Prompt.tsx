import React, { useState } from 'react';
import { KeyboardAvoidingView, Modal, Platform, ScrollView, View } from 'react-native';
import { Button, Field, Note, useUi } from './ui';

export interface PromptConfig {
  title: string;
  label: string;
  initial: string;
  submit: (value: string) => Promise<void>;
}
export function Prompt({ config, onClose }: { config: PromptConfig; onClose: () => void }) {
  const { colors, styles } = useUi();
  const [value, setValue] = useState(config.initial);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const run = async (action: () => Promise<void>) => {
    setBusy(true);
    setError(null);
    try {
      await action();
      onClose();
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : 'Could not save.');
    } finally {
      setBusy(false);
    }
  };
  return (
    <Modal
      transparent
      animationType="fade"
      visible
      onRequestClose={() => {
        if (!busy) onClose();
      }}
    >
      <KeyboardAvoidingView
        behavior={Platform.OS === 'ios' ? 'padding' : undefined}
        style={{ flex: 1, justifyContent: 'center', padding: 22, backgroundColor: colors.backdrop }}
      >
        <ScrollView
          keyboardShouldPersistTaps="handled"
          contentContainerStyle={{ flexGrow: 1, justifyContent: 'center' }}
        >
          <View accessibilityViewIsModal style={styles.card}>
            <Note>{config.title}</Note>
            <Field
              label={config.label}
              value={value}
              onChangeText={setValue}
              autoFocus
              autoCorrect={false}
              autoCapitalize="none"
              onSubmitEditing={() => {
                if (value.trim() && !busy) void run(() => config.submit(value.trim()));
              }}
            />
            {error && <Note error>{error}</Note>}
            <View style={styles.row}>
              <View style={styles.grow}>
                <Button title="Cancel" subtle disabled={busy} onPress={onClose} />
              </View>
              <View style={styles.grow}>
                <Button
                  title={busy ? 'Saving...' : 'Save'}
                  disabled={busy || !value.trim()}
                  onPress={() => {
                    void run(() => config.submit(value.trim()));
                  }}
                />
              </View>
            </View>
          </View>
        </ScrollView>
      </KeyboardAvoidingView>
    </Modal>
  );
}
