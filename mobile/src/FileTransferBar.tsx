import React from 'react';
import { Pressable, Text, View } from 'react-native';
import { Button, Note, useUi } from './ui';
import type { FileTransferProgress } from './file-transfer';

export function FolderIcon() {
  const { colors } = useUi();
  return <View accessible={false} style={{ width: 22, height: 20 }}>
    <View style={{ position: 'absolute', left: 2, top: 2, width: 9, height: 5, borderWidth: 1.5, borderBottomWidth: 0, borderColor: colors.text }} />
    <View style={{ position: 'absolute', left: 2, top: 6, width: 19, height: 13, borderWidth: 1.5, borderRadius: 1, borderColor: colors.text, backgroundColor: colors.bg }} />
  </View>;
}
export function FolderButton({ label = 'Files', disabled = false, onPress }: { label?: string; disabled?: boolean; onPress: () => void }) {
  const { colors } = useUi();
  return <Pressable accessibilityRole="button" accessibilityLabel={label} accessibilityState={{ disabled }} disabled={disabled} onPress={onPress}
    style={({ pressed }) => ({ width: 44, height: 44, alignItems: 'center', justifyContent: 'center', opacity: disabled ? 0.4 : pressed ? 0.6 : 1, backgroundColor: colors.panel, borderRadius: 3 })}>
    <FolderIcon />
  </Pressable>;
}
export function CopyFilePathButton({ disabled, onPress }: { disabled: boolean; onPress: () => void }) {
  const { colors } = useUi();
  return <Pressable accessibilityRole="button" accessibilityLabel="Copy file path" accessibilityState={{ disabled }} disabled={disabled} onPress={onPress}
    style={({ pressed }) => ({ width: 44, height: 44, alignItems: 'center', justifyContent: 'center', opacity: disabled ? 0.4 : pressed ? 0.6 : 1, backgroundColor: colors.panel, borderRadius: 3 })}>
    <View accessible={false} style={{ width: 22, height: 24 }}>
      <View style={{ position: 'absolute', left: 1, top: 1, width: 15, height: 17, borderWidth: 1.5, borderColor: colors.text, borderRadius: 1 }} />
      <View style={{ position: 'absolute', left: 6, top: 6, width: 15, height: 17, borderWidth: 1.5, borderColor: colors.text, borderRadius: 1, backgroundColor: colors.panel }} />
    </View>
  </Pressable>;
}
function TransferButton({ direction, disabled, onPress }: { direction: 'upload' | 'download'; disabled: boolean; onPress: () => void }) {
  const { colors } = useUi();
  return <Pressable accessibilityRole="button" accessibilityLabel={direction === 'download' ? 'Download selected file' : 'Upload to this folder'}
    accessibilityHint={direction === 'download' ? 'Save to Downloads, or export to Files on iOS.' : 'Choose a local file. Existing remote files are never replaced.'}
    accessibilityState={{ disabled }} disabled={disabled} onPress={onPress}
    style={({ pressed }) => ({ width: 44, height: 44, justifyContent: 'center', alignItems: 'center', borderWidth: 1, borderRadius: 3,
      borderColor: colors.line, backgroundColor: colors.panel, opacity: disabled ? 0.4 : pressed ? 0.6 : 1 })}>
    <View accessible={false} style={{ width: 24, height: 25 }}>
      <Text style={{ position: 'absolute', top: -5, left: 3, color: colors.text, fontSize: 25, lineHeight: 28 }}>{direction === 'download' ? '↓' : '↑'}</Text>
      <View style={{ position: 'absolute', left: 2, bottom: 0, width: 20, height: 6, borderLeftWidth: 1.5, borderRightWidth: 1.5, borderBottomWidth: 1.5, borderColor: colors.text }} />
    </View>
  </Pressable>;
}
function progressText(progress: FileTransferProgress): string {
  if (progress.phase === 'picking') return 'Choose a file…';
  if (progress.phase === 'exporting') return 'Saving download…';
  const percent = progress.size ? Math.floor(progress.offset / progress.size * 100) : 0;
  return `${progress.phase === 'uploading' ? 'Uploading' : 'Downloading'} ${progress.fileName} · ${percent}%`;
}
export function FileTransferBar({ canDownload, canUpload, download, upload, progress, message, error, cancel, dismiss }: {
  canDownload: boolean; canUpload: boolean; download: () => void; upload: () => void;
  progress: FileTransferProgress | null; message: string | null; error: string | null; cancel: () => void; dismiss: () => void;
}) {
  const { colors, styles } = useUi();
  return <View style={{ paddingHorizontal: 8, paddingBottom: 6, gap: 4 }}>
    <View style={styles.row}>
      <TransferButton direction="download" disabled={!canDownload || progress !== null} onPress={download} />
      <TransferButton direction="upload" disabled={!canUpload || progress !== null} onPress={upload} />
      <View style={[styles.grow, { minWidth: 48 }]}>{progress && <Text numberOfLines={2} accessibilityLiveRegion="polite" style={{ color: colors.muted, fontSize: 13 }}>{progressText(progress)}</Text>}</View>
      {progress && <Button title="Cancel transfer" subtle onPress={cancel} />}
    </View>
    {(message || error) && <View style={styles.row}><View style={styles.grow}><Note error={!!error}>{error ?? message}</Note></View><Button title="Dismiss transfer" subtle onPress={dismiss} /></View>}
  </View>;
}
