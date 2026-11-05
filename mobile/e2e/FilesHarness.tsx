// Isolated Android UI fixture. Never imported by the production entry.
import React, { useState } from 'react';
import { Pressable, Text, View } from 'react-native';
import { SafeAreaProvider } from 'react-native-safe-area-context';
import type { FileApi, FileRead } from '../../src/shared/file-contracts';
import { failure, success } from '../../src/shared/contracts';
import { FilesScreen } from '../src/FilesScreen';
import { ThemeProvider } from '../src/theme';
import { fixtureFileTransfers, transferFixtures, nativeLargeSize } from './FileTransferHarness';
import { downloads } from '../src/downloads';
import { nativeFixtureImage } from './image-fixture';
import Clipboard from '@react-native-clipboard/clipboard';

const revision = 'a'.repeat(64);
const image = nativeFixtureImage;
const icon = 'data:image/x-icon;base64,AAABAAEACAQAAAEAIABrAAAAFgAAAIlQTkcNChoKAAAADUlIRFIAAAAIAAAABAgGAAAAs81+8AAAACBJREFUeAGkyrENAAAMwrCIy/t5+QAGImWzOD4tSjswAAAA//9N8jKNAAAABklEQVQDAF/TDAFxcYBpAAAAAElFTkSuQmCC';
function dataSize(value: string) { const encoded = value.slice(value.indexOf(',') + 1); return encoded.length * 3 / 4 - (encoded.endsWith('==') ? 2 : encoded.endsWith('=') ? 1 : 0); }
const imageSize = dataSize(image);
const text = (name: string, mimeType: string, value: string): FileRead => ({ kind: 'text', path: `/fixture/${name}`, mimeType, text: value, size: value.length, revision });
const seed: FileRead[] = [
  text('code.ts', 'text/plain', 'const answer: number = 42;\nconsole.log(answer);\n// This deliberately long source line shows the default soft wrap on a narrow phone without forcing sideways scrolling or hiding the end of the code behind the edge of the screen.\n'),
  text('subdir/nested.txt', 'text/plain', 'Nested fixture'),
  text('README.md', 'text/markdown', '# Native Markdown\n\n![Fixture image](image.png)\n\n**Markdown sibling loaded**'),
  text('index.html', 'text/html', '<h1>Native HTML</h1><button onclick="this.textContent=String(window.fixtureValue)">Run preview</button><script src="local.js"></script>'),
  text('icon.svg', 'image/svg+xml', '<svg xmlns="http://www.w3.org/2000/svg" width="40" height="40"><rect width="40" height="40" fill="green"/></svg>'),
  { kind: 'image', path: '/fixture/image.png', mimeType: 'image/png', size: imageSize, revision, dataUrl: image },
  { kind: 'image', path: '/fixture/icon.ico', mimeType: 'image/x-icon', size: dataSize(icon), revision, dataUrl: icon },
];
export default function FilesHarness() {
  const [open, setOpen] = useState(false);
  const [hostOnly, setHostOnly] = useState(false);
  const [directoryApi] = useState(() => ({ getHomeDirectory: async () => success({ cwd: '/fixture' }),
    completeDirectory: async ({ path }: { path: string }) => success({ matches: path.startsWith('/fixture/s') ? ['/fixture/subdir/'] : [] }),
  }));
  const [status, setStatus] = useState('No saves');
  const [api] = useState<FileApi>(() => {
    const files = new Map([...seed, ...transferFixtures].map(file => [file.path, { ...file }]));
    return {
      ...fixtureFileTransfers(files, setStatus),
      list: async input => {
        if ((input.sessionId || input.tabId) && (input.sessionId !== '10000000-0000-4000-8000-000000000001' || input.tabId !== '10000000-0000-4000-8000-000000000002')) return failure('ACCESS_DENIED', 'Wrong fixture session/tab scope.');
        if (!['.', '~', '/fixture', '/fixture/subdir'].includes(input.path)) return failure('ACCESS_DENIED', 'Outside fixture folder.');
        const folder = input.path === '.' || input.path === '~' ? '/fixture' : input.path;
        return success({ path: folder, parentPath: folder === '/fixture' ? null : '/fixture', truncated: false,
          entries: [...Array.from(files.values()).filter(file => file.path.slice(0, file.path.lastIndexOf('/')) === folder)
            .map(file => ({ name: file.path.slice(folder.length + 1), path: file.path, kind: 'file' as const })),
          ...(folder === '/fixture' ? [{ name: 'subdir', path: '/fixture/subdir', kind: 'directory' as const }] : [])] });
      },
      read: async input => {
        if (input.path.endsWith('shellfox-transfer-large.bin')) return failure('UNSUPPORTED', `Fixture preview is too large: ${nativeLargeSize} bytes.`);
        const file = files.get(input.path);
        return file ? success(file) : failure('NOT_FOUND', 'Fixture file missing.');
      },
      write: async input => {
        const current = files.get(input.path);
        if (!current || current.kind !== 'text') return failure('NOT_FOUND', 'Fixture text missing.');
        if (input.revision !== current.revision) return failure('FILE_CONFLICT', 'Fixture changed.');
        const next = { ...current, text: input.text, size: input.text.length, revision: 'b'.repeat(64) };
        files.set(input.path, next); setStatus(`Saved text: ${input.text.trim()}`);
        return success({ path: next.path, size: next.size, revision: next.revision });
      },
      readAsset: async input => {
        setStatus(`Loaded asset: ${input.assetPath}`);
        return input.assetPath === 'image.png'
          ? success({ mimeType: 'image/png', size: imageSize, dataUrl: image })
          : input.assetPath === 'local.js' ? success({ mimeType: 'text/javascript', size: 1, dataUrl: 'data:text/javascript;base64,d2luZG93LmZpeHR1cmVWYWx1ZT0xMjM7' })
            : failure('NOT_FOUND', 'Fixture asset missing.');
      },
      writeImage: async input => {
        const current = files.get(input.path);
        if (!current || current.kind !== 'image') return failure('NOT_FOUND', 'Fixture image missing.');
        if (current.revision !== input.revision) return failure('FILE_CONFLICT', 'Fixture image changed.');
        const next = { ...current, revision: 'c'.repeat(64), dataUrl: input.dataUrl, size: dataSize(input.dataUrl) };
        files.set(input.path, next); setStatus('Saved image');
        return success({ path: next.path, revision: next.revision, size: next.size });
      },
    };
  });
  return <SafeAreaProvider><ThemeProvider><View style={{ flex: 1, paddingTop: 32, backgroundColor: '#121212' }}>
    <Pressable accessibilityRole="button" accessibilityLabel="Open files fixture" onPress={() => { setHostOnly(false); setOpen(true); }} style={{ padding: 16 }}>
      <Text style={{ color: '#eeeeee' }}>Open files fixture</Text>
    </Pressable>
    <Pressable accessibilityRole="button" accessibilityLabel="Open host files fixture" onPress={() => { setHostOnly(true); setOpen(true); }} style={{ padding: 16 }}>
      <Text style={{ color: '#eeeeee' }}>Open host files fixture</Text>
    </Pressable>
    <Pressable accessibilityRole="button" accessibilityLabel="Stage pending download fixture" onPress={() => {
      void downloads.begin('50000000-0000-4000-8000-000000000001', 'shellfox-transfer-abandoned.bin', 10, 'application/octet-stream')
        .then(async result => { if (result) { await downloads.write(result.handle, 0, 'YWJj'); setStatus('Staged pending download'); } }).catch(error => setStatus(String(error)));
    }} style={{ padding: 16 }}><Text style={{ color: '#eeeeee' }}>Stage pending download fixture</Text></Pressable>
    <Pressable accessibilityRole="button" accessibilityLabel="Read clipboard fixture" onPress={() => { void Clipboard.getString().then(value => setStatus(`Clipboard: ${value}`)); }} style={{ padding: 16 }}>
      <Text style={{ color: '#eeeeee' }}>Read clipboard fixture</Text>
    </Pressable>
    <Text accessibilityLabel={status} style={{ color: '#eeeeee' }}>{status}</Text>
    {open && <FilesScreen api={api} directoryApi={directoryApi} sessionId={hostOnly ? undefined : '10000000-0000-4000-8000-000000000001'}
      tabId={hostOnly ? undefined : '10000000-0000-4000-8000-000000000002'} available onClose={() => setOpen(false)} />}
  </View></ThemeProvider></SafeAreaProvider>;
}
