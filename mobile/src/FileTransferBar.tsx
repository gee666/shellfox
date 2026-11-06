import React from 'react';
import { Text, View } from 'react-native';
import { useSafeAreaInsets } from 'react-native-safe-area-context';
import { FileIconButton } from './FileIconButton';
import { useUi } from './ui';
import type { FileTransferProgress } from './file-transfer';

export function FolderButton({ label = 'Files', disabled = false, onPress }: { label?: string; disabled?: boolean; onPress: () => void }) {
  return <FileIconButton icon="folder" label={label} disabled={disabled} onPress={onPress} size={36} />;
}
function progressText(progress: FileTransferProgress): string {
  if (progress.phase === 'picking') return 'Choose a file…';
  if (progress.phase === 'exporting') return 'Saving download…';
  const percent = progress.size ? Math.floor(progress.offset / progress.size * 100) : 0;
  return `${progress.phase === 'uploading' ? 'Uploading' : 'Downloading'} ${progress.fileName} · ${percent}%`;
}
/** Transient feedback overlays content rather than reserving another navigation row. */
export function FileTransferBar({ progress, message, error, cancel, dismiss }: {
  progress: FileTransferProgress | null; message: string | null; error: boolean; cancel: () => void; dismiss: () => void;
}) {
  const { colors } = useUi();
  const insets = useSafeAreaInsets();
  if (!progress && !message) return null;
  return <View testID="files-notice" pointerEvents="box-none" style={{ position: 'absolute', left: Math.max(8, insets.left + 4), right: Math.max(8, insets.right + 4), bottom: insets.bottom + 8 }}>
    <View style={{ flexDirection: 'row', alignItems: 'center', gap: 6, paddingLeft: 10, paddingRight: 2, minHeight: 36,
      backgroundColor: colors.panel, borderWidth: 1, borderColor: error ? colors.danger : colors.line, borderRadius: 3 }}>
      <Text numberOfLines={2} accessibilityLiveRegion="polite" style={{ flex: 1, minWidth: 0, paddingVertical: 5, color: error ? colors.danger : colors.text, fontSize: 12 }}>
        {message ?? (progress ? progressText(progress) : '')}
      </Text>
      <FileIconButton icon="close" label={progress ? 'Cancel transfer' : 'Dismiss'} onPress={progress ? cancel : dismiss} />
    </View>
  </View>;
}
