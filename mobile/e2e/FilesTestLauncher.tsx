// Explicit QA entry only. The user-facing demo never imports this launcher.
import React, { useEffect, useState } from 'react';
import { Linking, Pressable, Text } from 'react-native';
import { SafeAreaProvider, SafeAreaView } from 'react-native-safe-area-context';
import { FilesScreen } from '../src/FilesScreen';
import { ThemeProvider } from '../src/theme';
import { downloads } from '../src/downloads';
import { createFilesFixture } from './files-fixture';
import { FilesQaMetrics } from './FilesQaMetrics';
import { useQaClipboard } from './useQaClipboard';

export default function FilesTestLauncher() {
  const [open, setOpen] = useState(false), [hostOnly, setHostOnly] = useState(false);
  const [status, setStatus] = useState('No saves');
  const [available, setAvailable] = useState(true);
  const clipboard = useQaClipboard();
  const [fixture] = useState(() => createFilesFixture(message => { setStatus(message); console.info(`FILES_QA_STATUS ${JSON.stringify(message)}`); }, { qa: true }));
  useEffect(() => {
    // Explicit component VIEW intents exercise offline close without adding normal app controls.
    const subscription = Linking.addEventListener('url', ({ url }) => {
      try {
        const command = new URL(url);
        if (command.protocol !== 'shellfox-files-qa:') return;
        if (command.hostname === 'disconnect') { setAvailable(false); setStatus('QA host disconnected'); }
        if (command.hostname === 'reconnect') { setAvailable(true); setStatus('QA host connected'); }
        if (command.hostname === 'cleanup') {
          const token = command.searchParams.get('token') ?? '';
          if (/^[a-z0-9-]{1,80}$/.test(token)) void clipboard.restore().catch(() => {}).then(() => console.info(`FILES_QA_CLEANUP ${token}`));
        }
      } catch { /* Ignore unrelated or malformed test intents. */ }
    });
    return () => subscription.remove();
  }, []);
  const button = (label: string, onPress: () => void) => <Pressable key={label} accessibilityRole="button" accessibilityLabel={label} onPress={onPress} style={{ padding: 14 }}><Text style={{ color: '#eeeeee' }}>{label}</Text></Pressable>;
  return <SafeAreaProvider><FilesQaMetrics /><ThemeProvider><SafeAreaView style={{ flex: 1, backgroundColor: '#121212' }}>
    <Text accessibilityLabel="Files QA launcher" style={{ color: '#eeeeee', padding: 14 }}>Files QA launcher</Text>
    {button('Open files fixture', () => { setAvailable(true); setHostOnly(false); setOpen(true); })}
    {button('Open host files fixture', () => { setAvailable(true); setHostOnly(true); setOpen(true); })}
    {button('Stage pending download fixture', () => {
      void downloads.begin('50000000-0000-4000-8000-000000000001', 'shellfox-transfer-abandoned.bin', 10, 'application/octet-stream')
        .then(async result => { if (result) { await downloads.write(result.handle, 0, 'YWJj'); setStatus('Staged pending download'); } }).catch(error => setStatus(String(error)));
    })}
    {button('Read clipboard fixture', () => { void clipboard.readFixture().then(setStatus); })}
    <Text accessibilityLabel={status} style={{ color: '#eeeeee', paddingHorizontal: 14 }}>{status}</Text>
    {open && <FilesScreen api={fixture.api} directoryApi={fixture.directoryApi} sessionId={hostOnly ? undefined : '10000000-0000-4000-8000-000000000001'}
      tabId={hostOnly ? undefined : '10000000-0000-4000-8000-000000000002'} available={available} onClose={() => setOpen(false)} />}
  </SafeAreaView></ThemeProvider></SafeAreaProvider>;
}
