import React, { useCallback, useEffect, useState } from 'react';
import { v4 as uuid } from 'uuid';
import { BackHandler, Platform, Pressable, StatusBar, Text, View } from 'react-native';
import { SafeAreaProvider, SafeAreaView } from 'react-native-safe-area-context';
import { useShellfox } from './src/useShellfox';
import { SessionsScreen, type OpenSession } from './src/SessionsScreen';
import { SettingsScreen } from './src/SettingsScreen';
import { SessionScreen } from './src/SessionScreen';
import { Prompt, type PromptConfig } from './src/Prompt';
import { Button, Note, colors, styles } from './src/ui';
import { updates } from './src/updates/native';

function Shellfox() {
  const model = useShellfox();
  const [section, setSection] = useState<'sessions' | 'settings'>('sessions');
  const [selection, setSelection] = useState<OpenSession | null>(null);
  const [prompt, setPrompt] = useState<{ id: string; config: PromptConfig } | null>(null);
  const showPrompt = useCallback((config: PromptConfig) => setPrompt({ id: uuid(), config }), []);
  const [updateVersion, setUpdateVersion] = useState<string | null>(null);
  useEffect(() => {
    if (Platform.OS !== 'android') return;
    let active = true;
    void updates
      .check()
      .then((update) => {
        if (active) setUpdateVersion(update?.version ?? null);
      })
      .catch(() => undefined);
    return () => {
      active = false;
    };
  }, []);
  const machine = model.machines.find((item) => item.peer.id === selection?.peerId);
  useEffect(() => {
    const handler = BackHandler.addEventListener('hardwareBackPress', () => {
      if (prompt) {
        setPrompt(null);
        return true;
      }
      if (selection) {
        setSelection(null);
        return true;
      }
      if (section !== 'sessions') {
        setSection('sessions');
        return true;
      }
      return false;
    });
    return () => handler.remove();
  }, [prompt, selection, section]);
  useEffect(() => {
    if (selection && model.ready && !machine) setSelection(null);
  }, [selection, model.ready, machine]);
  return (
    <SafeAreaView style={styles.page} edges={['top', 'left', 'right', 'bottom']}>
      <StatusBar barStyle="light-content" />
      {section === 'sessions' ? (
        selection && machine ? (
          <SessionScreen
            machine={machine}
            sessionId={selection.sessionId}
            model={model}
            back={() => setSelection(null)}
            prompt={showPrompt}
          />
        ) : (
          <SessionsScreen
            model={model}
            open={setSelection}
            prompt={showPrompt}
            settings={() => setSection('settings')}
          />
        )
      ) : (
        <SettingsScreen model={model} prompt={showPrompt} />
      )}
      {updateVersion && !selection && section === 'sessions' && (
        <View style={[styles.row, { padding: 12, backgroundColor: colors.panel }]}>
          <View style={styles.grow}>
            <Note>Shellfox {updateVersion} is available.</Note>
          </View>
          <Button title="View update" subtle onPress={() => setSection('settings')} />
        </View>
      )}
      {model.error && !(section === 'settings' && model.error === model.network.error) && (
        <View
          style={[
            styles.row,
            { padding: 12, backgroundColor: '#35252b', borderTopWidth: 1, borderColor: colors.line },
          ]}
        >
          <View style={styles.grow}>
            <Note error>{model.error}</Note>
          </View>
          <Button title="Dismiss" subtle onPress={() => model.setError(null)} />
        </View>
      )}
      {!selection && (
        <View
          accessibilityRole="tablist"
          style={{ flexDirection: 'row', borderTopWidth: 1, borderColor: colors.line }}
        >
          {(['sessions', 'settings'] as const).map((tab) => (
            <Pressable
              key={tab}
              testID={`tab-${tab}`}
              accessibilityRole="tab"
              accessibilityLabel={tab === 'sessions' ? 'Sessions' : 'Settings'}
              accessibilityState={{ selected: section === tab }}
              onPress={() => setSection(tab)}
              style={({ pressed }) => ({
                flex: 1,
                minHeight: 58,
                justifyContent: 'center',
                alignItems: 'center',
                backgroundColor: section === tab ? colors.panel : colors.bg,
                opacity: pressed ? 0.6 : 1,
              })}
            >
              <Text
                style={{
                  color: section === tab ? colors.accent : colors.muted,
                  fontSize: 14,
                  fontWeight: '600',
                }}
              >
                {tab === 'sessions' ? '>_  Sessions' : '⚙  Settings'}
              </Text>
            </Pressable>
          ))}
        </View>
      )}
      {prompt && (
        <Prompt
          key={prompt.id}
          config={prompt.config}
          onClose={() => setPrompt((value) => (value?.id === prompt.id ? null : value))}
        />
      )}
    </SafeAreaView>
  );
}
export default function App() {
  return (
    <SafeAreaProvider>
      <Shellfox />
    </SafeAreaProvider>
  );
}
