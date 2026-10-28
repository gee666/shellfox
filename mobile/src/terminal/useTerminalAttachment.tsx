import React, { useCallback, useEffect, useRef, useState } from 'react';
import { Alert, StyleSheet, Text, View } from 'react-native';
import Clipboard from '@react-native-clipboard/clipboard';
import { v4 as uuid } from 'uuid';
import { pickedFiles } from '../files';
import { useTheme } from '../theme';
import { AccessoryButton } from './KeyboardAccessory';
import { sameUploadIdentity, TerminalUpload, UploadCancelled, type TerminalUploadApi, type UploadedFile, type UploadIdentity, type UploadProgress } from './upload';

type Display = UploadProgress | { phase: 'done'; file: UploadedFile; copied: boolean } | { phase: 'error'; message: string };
interface Context { identity: UploadIdentity; current: () => boolean }
export function useTerminalAttachment(options: {
  api?: TerminalUploadApi;
  contextKey: string;
  enabled: boolean;
  getContext: () => Context | null;
  insert: (text: string) => void;
}) {
  const { colors } = useTheme();
  const [display, setDisplay] = useState<Display | null>(null);
  const transfer = useRef<TerminalUpload | null>(null);
  const context = useRef<Context | null>(null);
  const mounted = useRef(false);
  const currentOptions = useRef(options);
  currentOptions.current = options;
  const invalidate = useCallback(() => {
    transfer.current?.cancel(); transfer.current = null; context.current = null;
    if (mounted.current) setDisplay(null);
  }, []);
  useEffect(() => {
    mounted.current = true;
    return () => { mounted.current = false; invalidate(); };
  }, [options.api, options.contextKey, invalidate]);
  useEffect(() => { if (!options.enabled) invalidate(); }, [options.enabled, invalidate]);
  const start = () => {
    const { api, getContext, contextKey } = currentOptions.current;
    if (!api || transfer.current) return;
    const picked = getContext();
    if (!picked) return;
    const captured: Context = {
      identity: picked.identity,
      current: () => currentOptions.current.contextKey === contextKey && currentOptions.current.api === api
        && currentOptions.current.enabled && picked.current(),
    };
    context.current = captured;
    const upload = new TerminalUpload({
      api, files: pickedFiles, token: uuid(), identity: captured.identity,
      isCurrent: () => mounted.current && context.current === captured && captured.current(),
      onProgress: progress => { if (mounted.current && context.current === captured) setDisplay(progress); },
    });
    transfer.current = upload;
    void upload.run().then(file => {
      if (mounted.current && context.current === captured && captured.current()) setDisplay(file ? { phase: 'done', file, copied: false } : null);
    }).catch((error: unknown) => {
      if (mounted.current && context.current === captured && captured.current() && !(error instanceof UploadCancelled)) {
        setDisplay({ phase: 'error', message: error instanceof Error ? error.message : 'Could not attach the file.' });
      }
    }).finally(() => { if (transfer.current === upload) transfer.current = null; });
  };
  const available = () => {
    const captured = context.current;
    const live = currentOptions.current.getContext();
    return captured && captured.current() && live && sameUploadIdentity(live.identity, captured.identity);
  };
  const busy = display?.phase === 'picking' || display?.phase === 'uploading';
  const button = options.api ? <AccessoryButton label="Attach" onPress={start} disabled={!options.enabled || busy} /> : null;
  const panel = display && <View style={[styles.row, { backgroundColor: colors.panel, borderColor: colors.line }]}>
    <Text style={[styles.text, { color: display.phase === 'error' ? colors.danger : colors.text }]} numberOfLines={2} selectable={display.phase === 'done'}>
      {display.phase === 'picking' ? 'Choose a file or image...' : display.phase === 'uploading'
        ? `${display.fileName}  ${display.size === 0 ? 100 : Math.floor(display.offset / display.size * 100)}%`
        : display.phase === 'error' ? display.message : display.file.path}
    </Text>
    {display.phase === 'done' && <>
      <AccessoryButton label={display.copied ? 'Path copied' : 'Copy path'} onPress={() => {
        if (!available()) { invalidate(); return; }
        try { Clipboard.setString(display.file.path); setDisplay({ ...display, copied: true }); }
        catch { Alert.alert('Could not copy', 'The clipboard is unavailable. Try again.'); }
      }} />
      <AccessoryButton label="Insert path" onPress={() => {
        if (!available()) { invalidate(); return; }
        // Raw path for an agent prompt, not a shell command. Do not quote, escape or append Enter.
        currentOptions.current.insert(display.file.path);
      }} />
    </>}
    <AccessoryButton label={busy ? 'Cancel attachment' : 'Dismiss attachment'} onPress={invalidate}>
      <Text style={{ color: colors.text }}>{busy ? 'Cancel' : '×'}</Text>
    </AccessoryButton>
  </View>;
  return { button, panel, invalidate };
}
const styles = StyleSheet.create({
  row: { flexDirection: 'row', alignItems: 'center', gap: 4, borderTopWidth: 1, padding: 4 },
  text: { flex: 1, fontSize: 12 },
});
