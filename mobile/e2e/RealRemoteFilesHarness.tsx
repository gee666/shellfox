// Explicit QA entry. Real Windows disk + production RemoteClient; no enrollment or normal App import.
import React, { useEffect, useState } from 'react';
import { Linking, Text, View } from 'react-native';
import { SafeAreaProvider, SafeAreaView } from 'react-native-safe-area-context';
import { v4 as uuid } from 'uuid';
import { success } from '../../src/shared/contracts';
import type { FileApi } from '../../src/shared/file-contracts';
import { RemoteClient, type ClientState } from '../src/client';
import { FilesScreen } from '../src/FilesScreen';
import { ThemeProvider } from '../src/theme';
import { FileIconButton } from '../src/FileIconButton';
import { useUi } from '../src/ui';
import { createQaRemoteTransport, loadQaRemoteConfig, type QaRemoteConfig } from './remote-qa-transport';
import { useQaClipboard } from './useQaClipboard';

interface Runtime { client: RemoteClient; config: QaRemoteConfig; files: FileApi }
function RealRemoteFiles() {
  const { styles } = useUi();
  const clipboard = useQaClipboard();
  const [runtime, setRuntime] = useState<Runtime>();
  const [state, setState] = useState<ClientState>();
  const [opened, setOpened] = useState(false), [error, setError] = useState('');
  useEffect(() => {
    let live = true, client: RemoteClient | undefined, unsubscribe = () => {};
    void loadQaRemoteConfig().then(config => {
      if (!live) return;
      const remote = new RemoteClient(config.hostId, createQaRemoteTransport(config), { createClientId: uuid, approvalRetryMs: 400 });
      client = remote;
      const files: FileApi = { ...remote.files, list: input => remote.files.list({ ...input, path: input.path === '~' || input.path === '.' ? config.root : input.path }) };
      setRuntime({ client: remote, config, files });
      unsubscribe = remote.subscribeState(next => {
        if (!live) return; setState(next);
        console.info(`FILES_REAL_REMOTE_STATE ${next.status}`);
        if (next.status === 'connected') setOpened(true);
      });
      remote.connect();
    }).catch(cause => { if (live) setError(cause instanceof Error ? cause.message : 'Windows QA host unavailable.'); });
    const link = Linking.addEventListener('url', ({ url }) => {
      try {
        const command = new URL(url), token = command.searchParams.get('token') ?? '';
        if (command.protocol === 'shellfox-files-qa:' && command.hostname === 'cleanup' && /^[a-z0-9-]{1,80}$/.test(token)) void clipboard.restore().catch(() => {}).then(() => console.info(`FILES_QA_CLEANUP ${token}`));
      } catch { /* Ignore unrelated intents. */ }
    });
    return () => { live = false; link.remove(); unsubscribe(); client?.stop(); };
  }, []);
  return <SafeAreaView style={styles.page}>
    <View style={styles.header}><Text accessibilityLabel="Real remote host" style={styles.title}>Windows QA host</Text>
      {runtime && <FileIconButton icon="folder" label="Browse Windows files" disabled={state?.status !== 'connected'} onPress={() => setOpened(true)} />}</View>
    <View style={styles.content}><Text accessibilityLabel={`Remote state ${state?.status ?? 'starting'}`} style={styles.note}>{state?.status ?? 'Connecting to Windows QA host'}</Text>
      {error ? <Text accessibilityRole="alert" style={styles.note}>{error}</Text> : null}
      {runtime && <Text style={styles.note}>{runtime.config.root}</Text>}
    </View>
    {runtime && opened && <FilesScreen api={runtime.files} available={state?.status === 'connected'} connectionEpoch={state?.connectionId}
      hostName="Windows remote QA" directoryApi={{ getHomeDirectory: async () => success({ cwd: runtime.config.root }), completeDirectory: input => runtime.client.completeDirectory(input) }}
      onClose={() => setOpened(false)} />}
  </SafeAreaView>;
}
export default function RealRemoteFilesHarness() {
  return <SafeAreaProvider><ThemeProvider><RealRemoteFiles /></ThemeProvider></SafeAreaProvider>;
}
