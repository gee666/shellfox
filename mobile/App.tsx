import React, { useCallback, useEffect, useState } from 'react';
import { v4 as uuid } from 'uuid';
import { BackHandler, Pressable, StatusBar, Text, View } from 'react-native';
import { SafeAreaProvider, SafeAreaView } from 'react-native-safe-area-context';
import { useShellfox, type ShellfoxModel } from './src/useShellfox';
import { SessionsScreen, type OpenSession } from './src/SessionsScreen';
import { SettingsScreen } from './src/SettingsScreen';
import { SessionScreen } from './src/SessionScreen';
import { Prompt, type PromptConfig } from './src/Prompt';
import { Button, Note, useUi } from './src/ui';
import { ThemeProvider, useTheme } from './src/theme';
import { useUpdates } from './src/updates/useUpdates';

function Shellfox() {
  const model = useShellfox();
  return (
    <ThemeProvider accentColor={model.preferences.accentColor} backgroundColor={model.preferences.backgroundColor}>
      <ShellfoxView model={model} />
    </ThemeProvider>
  );
}

function ShellfoxView({ model }: { model: ShellfoxModel }) {
  const { colors, styles } = useUi();
  const { palette } = useTheme();
  const [section, setSection] = useState<'sessions' | 'settings'>('sessions');
  const [selection, setSelection] = useState<OpenSession | null>(null);
  const [prompt, setPrompt] = useState<{ id: string; config: PromptConfig } | null>(null);
  const showPrompt = useCallback((config: PromptConfig) => setPrompt({ id: uuid(), config }), []);
  const updateStatus = useUpdates(model.preferencesLoaded, model.preferences.autoUpdate);
  const updateVersion = updateStatus.version;
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
      <StatusBar barStyle={palette.scheme === 'dark' ? 'light-content' : 'dark-content'} />
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
        <SettingsScreen model={model} prompt={showPrompt} updateStatus={updateStatus} />
      )}
      {updateVersion && !selection && section === 'sessions' && (
        <View style={[styles.row, { padding: 12, backgroundColor: colors.panel }]}>
          <View style={styles.grow}>
            <Note>{updateStatus.message ?? `Shellfox ${updateVersion} is available.`}</Note>
          </View>
          <Button title="View update" subtle onPress={() => setSection('settings')} />
        </View>
      )}
      {model.error && !(section === 'settings' && model.error === model.network.error) && (
        <View
          style={[
            styles.row,
            { padding: 12, backgroundColor: colors.panel, borderTopWidth: 1, borderColor: colors.danger },
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
