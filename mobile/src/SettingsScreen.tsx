import React, { useState } from 'react';
import { Alert, AppState, Linking, Platform, ScrollView, Text, View } from 'react-native';
import { remoteHostnameSchema } from '../../src/shared/remote-schemas';
import type { ShellfoxModel } from './useShellfox';
import { Button, Card, Field, Note, styles } from './ui';
import type { PromptConfig } from './Prompt';
import { updates } from './updates/native';

export function SettingsScreen({
  model,
  prompt,
}: {
  model: ShellfoxModel;
  prompt: (value: PromptConfig) => void;
}) {
  const [authKey, setAuthKey] = useState('');
  const [busy, setBusy] = useState(false);
  const [updateMessage, setUpdateMessage] = useState<string | null>(null);
  const [updateAvailable, setUpdateAvailable] = useState(false);
  const run = (action: () => Promise<unknown>) => {
    setBusy(true);
    void action()
      .catch(model.report)
      .finally(() => setBusy(false));
  };
  const openLogin = async (raw: string) => {
    const url = new URL(raw);
    if (url.origin !== 'https://login.tailscale.com' || url.username || url.password)
      throw new Error('Invalid Tailscale sign-in URL.');
    if (AppState.currentState === 'active') await Linking.openURL(url.href);
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
        <Text style={styles.subtitle}>Shellfox mobile</Text>
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
          <Note>
            Built in. No separate VPN app needed. Sign in with the same Tailscale account as your host.
          </Note>
          {model.network.error && <Note error>{model.network.error}</Note>}
          {!model.ready && model.error && (
            <Button title="Retry startup" subtle disabled={busy} onPress={() => run(model.retryStart)} />
          )}
          {model.network.status !== 'online' && (
            <>
              <Button
                title="Sign in with browser"
                disabled={busy || !model.ready}
                onPress={() =>
                  run(async () => {
                    const url = model.network.loginUrl ?? (await model.login()).loginUrl;
                    if (url) await openLogin(url);
                  })
                }
              />
              <Field
                label="Or use a Tailscale auth key"
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
        <Text style={styles.section}>Machines</Text>
        {!model.peers.length && (
          <Note>
            {model.network.status === 'online'
              ? 'No machines found. Enable remote access in Shellfox on your computer.'
              : 'Sign in to see machines on your tailnet.'}
          </Note>
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
                    {peer.online ? 'Online' : 'Offline'}
                    {machine ? ` · ${machine.state.status.replaceAll('-', ' ')}` : ''}
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
          <Note>
            Long-press terminal text to select and copy. Use the key row for control shortcuts and navigation.
          </Note>
        </Card>
        {Platform.OS === 'android' && (
          <>
            <Text style={styles.section}>Updates</Text>
            <Card>
              <Note>
                Install updates directly from this repository's GitHub releases. Android will ask you to
                confirm the installation.
              </Note>
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
        <Note>
          Client only. Sessions and processes run on your hosts, not on this device. Leaving this app does not
          stop them.
        </Note>
      </ScrollView>
    </View>
  );
}
