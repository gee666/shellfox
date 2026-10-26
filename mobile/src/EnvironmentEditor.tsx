import React, { useState } from 'react';
import { KeyboardAvoidingView, Modal, Platform, ScrollView, Text, View } from 'react-native';
import type { EnvVar, SessionDto } from '../../src/shared/contracts';
import { envVarsSchemaFor } from '../../src/shared/schemas';
import { Button, Field, Note, useUi } from './ui';

export function EnvironmentEditor({
  session,
  save,
  close,
}: {
  session: SessionDto;
  save: (env: EnvVar[]) => Promise<void>;
  close: () => void;
}) {
  const { styles } = useUi();
  const [env, setEnv] = useState(session.env);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const update = (index: number, patch: Partial<EnvVar>) =>
    setEnv((items) => items.map((item, i) => (i === index ? { ...item, ...patch } : item)));
  return (
    <Modal visible animationType="slide" onRequestClose={close}>
      <KeyboardAvoidingView behavior={Platform.OS === 'ios' ? 'padding' : undefined} style={styles.page}>
        <View style={[styles.content, { flex: 1, paddingTop: 48 }]}>
          <Text style={styles.title}>Environment</Text>
          <Note>
            Applies to new terminals.
          </Note>
          <ScrollView keyboardShouldPersistTaps="handled" contentContainerStyle={{ gap: 14 }}>
            {env.map((item, index) => (
              <View key={index} style={styles.card}>
                <Field
                  label={`Variable ${index + 1}`}
                  value={item.name}
                  autoCapitalize="none"
                  autoCorrect={false}
                  onChangeText={(name) => update(index, { name })}
                  editable={!busy}
                />
                <Field
                  label="Value"
                  value={item.value}
                  autoCapitalize="none"
                  autoCorrect={false}
                  onChangeText={(value) => update(index, { value })}
                  editable={!busy}
                />
                <Button
                  title="Remove"
                  subtle
                  disabled={busy}
                  onPress={() => setEnv((items) => items.filter((_, i) => i !== index))}
                />
              </View>
            ))}
            <Button
              title="Add variable"
              subtle
              disabled={busy || env.length >= 200}
              onPress={() => setEnv((items) => [...items, { name: '', value: '' }])}
            />
          </ScrollView>
          {error && <Note error>{error}</Note>}
          <View style={styles.row}>
            <View style={styles.grow}>
              <Button title="Cancel" subtle disabled={busy} onPress={close} />
            </View>
            <View style={styles.grow}>
              <Button
                title={busy ? 'Saving...' : 'Save'}
                disabled={busy}
                onPress={() => {
                  const parsed = envVarsSchemaFor(false).safeParse(env);
                  if (!parsed.success) {
                    setError(parsed.error.issues[0]?.message ?? 'Check variable names and values.');
                    return;
                  }
                  setBusy(true);
                  setError(null);
                  void save(parsed.data)
                    .then(close)
                    .catch((cause) => setError(cause instanceof Error ? cause.message : 'Could not save.'))
                    .finally(() => setBusy(false));
                }}
              />
            </View>
          </View>
        </View>
      </KeyboardAvoidingView>
    </Modal>
  );
}
