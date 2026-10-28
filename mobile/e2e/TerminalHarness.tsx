// Explicit native-only regression fixture. No production entry imports this file.
import React, { useState } from 'react';
import { Pressable, Text, View } from 'react-native';
import { SafeAreaProvider } from 'react-native-safe-area-context';
import { success, type TerminalEvent } from '../../src/shared/contracts';
import { TerminalView } from '../src/terminal/TerminalView';
import type { MobileTerminalApi, SubscribeTerminal } from '../src/terminal/bridge';
import { DirectoryHarness } from './DirectoryHarness';
import { fixtureUploads } from './UploadHarness';

const tabId = '12345678-1234-4234-8234-123456789abc';
const generation = '12345678-1234-4234-8234-123456789abd';
const replay = '\x1b[32mNative terminal: café 日本語\x1b[0m\r\n$ ';
export default function TerminalHarness() {
  const [mounted, setMounted] = useState(false);
  const [directory, setDirectory] = useState(false);
  const [status, setStatus] = useState('Waiting');
  const [delivery, setDelivery] = useState('Delivery 0/1');
  const [uploadStatus, setUploadStatus] = useState('No attachment');
  const [uploadApi] = useState(() => fixtureUploads(setUploadStatus));
  const [fixture] = useState(() => {
    let emit: (event: TerminalEvent) => void = () => {};
    let sequence = 1, input = '', grid = '', rendered = 0;
    const update = () => {
      const escaped = input.replace(/[\x00-\x1f\x7f]/g, value => `\\x${value.charCodeAt(0).toString(16).padStart(2, '0')}`);
      setStatus(`Rendered ${rendered}; grid ${grid}; input ${escaped}`);
      setDelivery(`Delivery ${rendered}/${sequence}`);
    };
    const api: MobileTerminalApi = {
      attachTerminal: async () => success({ control: 'owned', controlEpoch: 1, tabId, sessionId: tabId, generation, firstSequence: sequence,
        lastSequence: sequence, chunks: [{ type: 'data', tabId, generation, sequence, data: replay + input }],
        snapshot: true, truncated: sequence > 1, cols: 80, rows: 24, lifetime: 'app-owned', state: 'open', exitCode: null }),
      writeTerminal: async request => {
        input += request.data;
        emit({ type: 'data', tabId, generation, controlEpoch: 1, sequence: ++sequence, data: request.data });
        update(); return success({ written: true });
      },
      resizeTerminal: async request => { grid = `${request.cols}x${request.rows}`; update(); return success({ resized: true }); },
      acknowledgeTerminal: async request => { rendered = request.sequence; update(); return success({ acknowledged: true }); },
      detachTerminal: async () => success({ detached: true }),
      takeTerminalControl: async () => success({ taken: true }),
    };
    const subscribeTerminal: SubscribeTerminal = listener => { emit = listener; return () => { emit = () => {}; }; };
    return { api, subscribeTerminal };
  });
  return <SafeAreaProvider><View style={{ flex: 1, backgroundColor: '#111016', paddingTop: 30 }}>
    <Pressable accessibilityRole="button" accessibilityLabel="Toggle terminal fixture"
      onPress={() => setMounted(value => !value)} style={{ padding: 8 }}>
      <Text style={{ color: '#ffffff' }}>Terminal native test</Text>
    </Pressable>
    <Pressable accessibilityRole="button" accessibilityLabel="Directory fixture"
      onPress={() => { setMounted(false); setDirectory(value => !value); }} style={{ padding: 8 }}>
      <Text style={{ color: '#ffffff' }}>Directory fixture</Text>
    </Pressable>
    {directory && <DirectoryHarness />}
    <Text style={{ color: '#ffffff', fontSize: 10 }}>{status}</Text>
    <Text style={{ color: '#ffffff', fontSize: 10 }}>{delivery}</Text>
    <Text accessibilityLabel={uploadStatus} style={{ color: '#ffffff', fontSize: 10 }}>{uploadStatus}</Text>
    {mounted && <TerminalView {...fixture} uploadApi={uploadApi} tabId={tabId} fontSize={14} connectionEpoch="fixture"
      onError={error => setStatus(`ERROR ${error.message}`)} />}
  </View></SafeAreaProvider>;
}
