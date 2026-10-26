import React, { useState } from 'react';
import { Alert, Platform, ScrollView, Text, View } from 'react-native';
import { remoteHostnameSchema } from '../../src/shared/remote-schemas';
import type { ShellfoxModel } from './useShellfox';
import { Button, Card, Field, Note, useUi } from './ui';
import { ColorSwatches } from './ColorSwatches';
import { DEFAULT_ACCENT, DEFAULT_BACKGROUND } from './theme';
import type { PromptConfig } from './Prompt';
import { updates } from './updates/native';
import { useBrowserLogin } from './browser-login';
import { StatusIndicator } from './StatusIndicator';

export function SettingsScreen({
  model,
  prompt,
}: {
  model: ShellfoxModel;
  prompt: (value: PromptConfig) => void;
}) {
  const { styles } = useUi();
  const [authKey, setAuthKey] = useState('');
  const [actionBusy, setBusy] = useState(false);
  const browser = useBrowserLogin(model);
  const busy = actionBusy || browser.pending;
  const [updateMessage, setUpdateMessage] = useState<string | null>(null);
  const [updateAvailable, setUpdateAvailable] = useState(false);
  const run = (action: () => Promise<unknown>) => {
    setBusy(true);
    void action()
      .catch(model.report)
      .finally(() => setBusy(false));
  };
  const signIn = () =>
    run(async () => {
      const key = authKey.trim();
      setAuthKey('');
      await model.login(key);
    });
  const checkUpdate = () =>
    run(async () => {
      setUpdateMessage('Checking GitHub...');
      try {
        const update = await updates.check();
        setUpdateAvailable(update !== null);
        setUpdateMessage(update ? `Version ${update.version} is available.` : 'You have the latest release.');
      } catch (cause) {
        setUpdateMessage('Could not check for updates. Try again.');
        throw cause;
      }
    });
  return (
    <View style={styles.page}>
      <View style={styles.header}>
        <Text style={styles.title}>Settings</Text>
      </View>
      <ScrollView contentContainerStyle={styles.content} keyboardShouldPersistTaps="handled">
        <Text style={styles.section}>Tailscale</Text>
        <Card>
          <View style={styles.row}>
            <View style={styles.grow}>
              <Text style={styles.label}>{model.preferences.hostname}</Text>
              <Note>{model.network.status.replaceAll('-', ' ')}</Note>
            </View>
            <Button
              title="Rename"
              subtle
              disabled={busy}
              onPress={() =>
                prompt({
                  title: 'Device name',
                  label: 'Hostname',
                  initial: model.preferences.hostname,
                  submit: async (hostname) => {
                    await model.renameDevice(remoteHostnameSchema.parse(hostname));
                  },
                })
              }
            />
          </View>
          {model.network.error && <Note error>{model.network.error}</Note>}
          {!model.ready && model.error && (
            <Button title="Retry startup" subtle disabled={busy} onPress={() => run(model.retryStart)} />
          )}
          {model.network.status !== 'online' && (
            <>
              <Button
                title={browser.pending ? 'Opening browser...' : 'Sign in with browser'}
                disabled={busy || !model.ready}
                onPress={browser.start}
              />
              <Field
                label="Auth key"
                placeholder="tskey-auth-..."
                secureTextEntry
                value={authKey}
                onChangeText={setAuthKey}
                autoCapitalize="none"
                autoCorrect={false}
                textContentType="none"
              />
              <Button
                title="Use auth key"
                subtle
                disabled={busy || !authKey.trim() || !model.ready}
                onPress={signIn}
              />
            </>
          )}
          {model.network.status === 'online' && (
            <Button
              title="Sign out"
              subtle
              disabled={busy}
              onPress={() =>
                Alert.alert(
                  'Sign out of Tailscale?',
                  'Remote shells will keep running. You can sign in again to reconnect.',
                  [
                    { text: 'Cancel', style: 'cancel' },
                    { text: 'Sign out', style: 'destructive', onPress: () => run(model.logout) },
                  ],
                )
              }
            />
          )}
        </Card>
        {model.network.status === 'online' && <Text style={styles.section}>Machines</Text>}
        {!model.peers.length && model.network.status === 'online' && (
          model.loading ? <StatusIndicator loading label="Finding hosts" /> : <Note>No hosts</Note>
        )}
        {model.peers.map((peer) => {
          const machine = model.machines.find((item) => item.peer.id === peer.id);
          const remembered = model.preferences.remembered.includes(peer.id);
          return (
            <Card key={peer.id}>
              <View style={styles.row}>
                <View style={styles.grow}>
                  <Text style={styles.label}>{peer.hostname}</Text>
                  <Note>
                    {machine ? machine.state.status.replaceAll('-', ' ') : peer.online ? 'online' : 'offline'}
                  </Note>
                </View>
                <Button
                  title={remembered ? 'Disconnect' : 'Connect'}
                  subtle
                  disabled={busy || (!peer.online && !remembered)}
                  onPress={() => run(() => (remembered ? model.disconnect(peer.id) : model.connect(peer)))}
                />
              </View>
              {machine?.state.status === 'awaiting-approval' && (
                <Note>Approve {model.preferences.hostname} in the host's Shellfox remote settings.</Note>
              )}
              {machine?.state.lastError && machine.state.status !== 'awaiting-approval' && (
                <Note error>{machine.state.lastError.message}</Note>
              )}
              {machine && !remembered && (
                <Button
                  title="Forget machine"
                  subtle
                  onPress={() => run(() => model.disconnect(peer.id, true))}
                />
              )}
            </Card>
          );
        })}
        <Text style={styles.section}>Colors</Text>
        <Card>
          <ColorSwatches
            kind="accent"
            value={model.preferences.accentColor ?? DEFAULT_ACCENT}
            saved={model.preferences.customAccentColors ?? []}
            disabled={busy}
            onChange={accentColor => run(() => model.updatePreferences({ accentColor }))}
            onSave={customAccentColors => run(() => model.updatePreferences({ customAccentColors }))}
          />
          <ColorSwatches
            kind="background"
            value={model.preferences.backgroundColor ?? DEFAULT_BACKGROUND}
            saved={model.preferences.customBackgroundColors ?? []}
            disabled={busy}
            onChange={backgroundColor => run(() => model.updatePreferences({ backgroundColor }))}
            onSave={customBackgroundColors => run(() => model.updatePreferences({ customBackgroundColors }))}
          />
        </Card>
        <Text style={styles.section}>Terminal</Text>
        <Card>
          <View style={styles.row}>
            <View style={styles.grow}>
              <Text style={styles.label}>Text size</Text>
              <Note>{model.preferences.fontSize} px</Note>
            </View>
            <Button
              title="−"
              subtle
              disabled={model.preferences.fontSize <= 10}
              onPress={() => run(() => model.updatePreferences({ fontSize: model.preferences.fontSize - 1 }))}
            />
            <Button
              title="+"
              subtle
              disabled={model.preferences.fontSize >= 24}
              onPress={() => run(() => model.updatePreferences({ fontSize: model.preferences.fontSize + 1 }))}
            />
          </View>
        </Card>
        {Platform.OS === 'android' && (
          <>
            <Text style={styles.section}>Updates</Text>
            <Card>
              {updateMessage && <Note>{updateMessage}</Note>}
              <Button title="Check for updates" subtle disabled={busy} onPress={checkUpdate} />
              {updateAvailable && (
                <Button
                  title="Download and install"
                  disabled={busy}
                  onPress={() =>
                    run(async () => {
                      setUpdateMessage('Downloading and verifying APK...');
                      const installed = await updates.install();
                      setUpdateMessage(
                        installed
                          ? 'Confirm the update in the Android installer.'
                          : 'No newer release is available.',
                      );
                      if (!installed) setUpdateAvailable(false);
                    })
                  }
                />
              )}
            </Card>
          </>
        )}
      </ScrollView>
    </View>
  );
}
