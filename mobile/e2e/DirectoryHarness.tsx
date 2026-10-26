// Explicit native-only Modal/IME fixture. No production entry imports this file.
import React, { useState } from 'react';
import { Text, View } from 'react-native';
import { failure, success } from '../../src/shared/contracts';
import { DirectoryPrompt, type DirectoryPromptConfig } from '../src/DirectoryPrompt';
import { ThemeProvider } from '../src/theme';
import { Button } from '../src/ui';

const home = '/home/mobile';
function fixturePath(input: string): string {
  const path = input.trim().replace(/\/$/, '');
  return path === '~' ? home : path.startsWith('~/') ? home + path.slice(1) : path;
}
export function createDirectoryFixtureApi(onCreate: (path: string) => void): DirectoryPromptConfig['api'] {
  const folders = new Set([home, home + '/work', home + '/workshop', home + '/data', home + '/work/src']);
  return {
    getHomeDirectory: async () => success({ cwd: '~/' }),
    completeDirectory: async ({ path }) => {
      const input = path.trim();
      const split = input.lastIndexOf('/');
      const parent = fixturePath(split < 0 ? '~' : input.slice(0, split + 1));
      const prefix = input.slice(split + 1);
      const displayParent = split < 0 ? '' : input.slice(0, split + 1);
      const matches = [...folders].filter(folder => {
        const relative = folder.slice(parent.length + 1);
        return folder.startsWith(parent + '/') && !relative.includes('/') && relative.startsWith(prefix);
      }).map(folder => displayParent + folder.slice(parent.length + 1) + '/').sort();
      return success({ matches });
    },
    resolveDirectory: async ({ path }) => {
      const cwd = fixturePath(path);
      return folders.has(cwd) ? success({ cwd }) : failure('NOT_FOUND', 'Directory does not exist.');
    },
    createDirectory: async ({ path }) => {
      const cwd = fixturePath(path);
      if (!cwd.startsWith(home + '/')) return failure('VALIDATION', 'Use a folder under ~/ in this fixture.');
      const segments = cwd.slice(home.length + 1).split('/');
      for (let index = 1; index <= segments.length; index++) folders.add(home + '/' + segments.slice(0, index).join('/'));
      onCreate(cwd);
      return success({ cwd });
    },
  };
}
export function DirectoryHarness() {
  const [status, setStatus] = useState('Directory fixture ready; sessions 0; folders 0');
  const [visible, setVisible] = useState(true);
  const [dialog, setDialog] = useState(1);
  const [fixture] = useState(() => {
    const counts = { sessions: 0, folders: 0 };
    return { counts, api: createDirectoryFixtureApi(() => { counts.folders++; }) };
  });
  const config: DirectoryPromptConfig = {
    id: `directory-fixture-${dialog}`,
    title: 'New session fixture',
    api: fixture.api,
    submit: async (cwd, signal) => {
      if (signal.aborted) return;
      fixture.counts.sessions++;
      setStatus(`Created session ${cwd}; sessions ${fixture.counts.sessions}; folders ${fixture.counts.folders}`);
    },
  };
  return <ThemeProvider><View>
    <Text accessibilityLabel={status} style={{ color: '#ffffff', fontSize: 12 }}>{status}</Text>
    <Button title="Open directory fixture" onPress={() => { setDialog(value => value + 1); setVisible(true); }} />
    {visible && <DirectoryPrompt config={config} onClose={() => setVisible(false)} />}
  </View></ThemeProvider>;
}
