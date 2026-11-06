// User-facing isolated demo. QA tools live in the separate files-qa-entry.js bundle.
import React, { useState } from 'react';
import { Text, View } from 'react-native';
import { SafeAreaProvider, SafeAreaView } from 'react-native-safe-area-context';
import { FilesScreen } from '../src/FilesScreen';
import { ThemeProvider } from '../src/theme';
import { useUi } from '../src/ui';
import { FileIconButton } from '../src/FileIconButton';
import { createFilesFixture } from './files-fixture';

function FilesDemo() {
  const [open, setOpen] = useState(true);
  const [fixture] = useState(() => createFilesFixture());
  const { colors, styles } = useUi();
  return <SafeAreaView style={styles.page}>
    <View style={styles.header}><Text style={styles.title}>Shellfox</Text></View>
    <View style={{ flexDirection: 'row', alignItems: 'center', padding: 8, borderBottomWidth: 1, borderColor: colors.line }}>
      <View style={{ flex: 1 }}><Text style={styles.label}>Demo workspace</Text><Text style={styles.subtitle}>/fixture</Text></View>
      <FileIconButton icon="folder" label="Browse files" onPress={() => setOpen(true)} />
    </View>
    {open && <FilesScreen api={fixture.api} directoryApi={fixture.directoryApi} available onClose={() => setOpen(false)} />}
  </SafeAreaView>;
}
export default function FilesHarness() {
  return <SafeAreaProvider><ThemeProvider><FilesDemo /></ThemeProvider></SafeAreaProvider>;
}
