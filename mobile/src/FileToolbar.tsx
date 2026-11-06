import React from 'react';
import { Text, View, type ModalProps } from 'react-native';
import { FileIconButton } from './FileIconButton';
import { useUi } from './ui';

export const FILE_TOOLBAR_HEIGHT = 36;
export const FILE_MODAL_ORIENTATIONS: NonNullable<ModalProps['supportedOrientations']> = ['portrait', 'portrait-upside-down', 'landscape-left', 'landscape-right'];
/** Six contextual actions leave a useful filename even at 320dp. */
export function FileToolbar({ title, file, editable, editing, text, dirty, saving, blocked, canTransfer, canCopy, canSave,
  close, toggleMode, path, copy, download, upload, refresh, more, save,
}: {
  title: string; file: boolean; editable: boolean; editing: boolean; text: boolean; dirty: boolean;
  saving: boolean; blocked: boolean; canTransfer: boolean; canCopy: boolean; canSave: boolean;
  close: () => void; toggleMode: () => void; path: () => void; copy: () => void;
  download: () => void; upload: () => void; refresh: () => void; more: () => void; save: () => void;
}) {
  const { colors } = useUi();
  const saveContext = file && text && (editing || dirty);
  return <View testID="files-toolbar" collapsable={false}
    style={{ height: FILE_TOOLBAR_HEIGHT, flexDirection: 'row', alignItems: 'center', gap: 2, paddingHorizontal: 4,
      backgroundColor: colors.bg, borderBottomWidth: 1, borderColor: colors.line }}>
    <FileIconButton testID="files-close" icon={file ? 'close' : 'back'} label={file ? 'Close file' : 'Back'} onPress={close} disabled={saving} />
    <Text testID="files-title" numberOfLines={1} accessibilityLabel={`${title}${dirty ? ', unsaved changes' : ''}`}
      style={{ flex: 1, minWidth: 0, color: colors.text, fontSize: 13, fontWeight: '500' }}>{dirty ? '*' : ''}{title}</Text>
    {file && editable && <FileIconButton testID="files-mode" icon={editing ? 'preview' : 'edit'} label={editing ? 'Read file' : text ? 'Edit source' : 'Edit image'}
      active={editing} disabled={blocked} onPress={toggleMode} />}
    <FileIconButton testID="files-path" icon="folder" label={file ? 'Show file path' : 'Change directory'} onPress={path} disabled={blocked} />
    {file && <FileIconButton testID="files-copy" icon="copy" label="Copy file path" onPress={copy} disabled={!canCopy} />}
    {!editing && !saveContext && <FileIconButton testID={file ? 'files-download' : 'files-upload'} icon={file ? 'download' : 'upload'} label={file ? 'Download selected file' : 'Upload to this folder'}
      onPress={file ? download : upload} disabled={!canTransfer || blocked} />}
    {!file && <FileIconButton testID="files-refresh" icon="reload" label="Refresh" onPress={refresh} disabled={blocked} />}
    <FileIconButton testID="files-more" icon="more" label={file ? 'More file actions' : 'More folder actions'} onPress={more} />
    {saveContext && <FileIconButton testID="files-save" icon="save" label={saving ? 'Saving' : 'Save'} active={dirty} busy={saving} disabled={!dirty || blocked || !canSave} onPress={save} />}
  </View>;
}
