import React, { useEffect, useRef, useState } from 'react';
import { FlatList, Keyboard, Modal, Pressable, Text, View } from 'react-native';
import { SafeAreaProvider, SafeAreaView } from 'react-native-safe-area-context';
import type { ManagerApi } from '../../src/shared/contracts';
import type { BrowserDirectory, FileApi, FileOpenRequest } from './file-browser-types';
import { FileContent } from './FileContent';
import { FileIconGlyph } from './FileIconButton';
import { FileMoreMenu, type FileMenuItem } from './FileMoreMenu';
import { FilePathSheet } from './FilePathSheet';
import { FileToolbar, FILE_MODAL_ORIENTATIONS } from './FileToolbar';
import { FileTransferBar } from './FileTransferBar';
import { remoteFileName } from './file-transfer';
import { textPreviewKind } from './file-preview-document';
import { useFileBrowser } from './useFileBrowser';
import { Note, useUi } from './ui';

type Entry = BrowserDirectory['entries'][number];
export interface FilesScreenProps {
  api: FileApi;
  sessionId?: string;
  tabId?: string;
  directoryApi?: Pick<ManagerApi, 'getHomeDirectory' | 'completeDirectory'>;
  connectionEpoch?: string | null;
  hostName?: string;
  openRequest?: FileOpenRequest;
  available: boolean;
  onClose: () => void;
}
export function FilesScreen(props: FilesScreenProps) {
  const back = useRef(props.onClose);
  return <Modal visible animationType="slide" presentationStyle="fullScreen" supportedOrientations={FILE_MODAL_ORIENTATIONS} statusBarTranslucent navigationBarTranslucent onRequestClose={() => back.current()}>
    <SafeAreaProvider><FilesView {...props} backHandler={back} /></SafeAreaProvider>
  </Modal>;
}
function FilesView(props: FilesScreenProps & { backHandler: React.RefObject<() => void> }) {
  const { colors, styles } = useUi();
  const browser = useFileBrowser(props);
  const [pathOpen, setPathOpen] = useState(false);
  const [menuOpen, setMenuOpen] = useState(false);
  const [offlineDismissed, setOfflineDismissed] = useState(false);
  const list = useRef<FlatList<Entry>>(null);
  useEffect(() => { if (props.available) setOfflineDismissed(false); }, [props.available]);
  const { file, fileOpen, directory, blocked, mode } = browser;
  const text = file?.kind === 'text';
  const editable = text || file?.kind === 'image' && /^(?:image\/png|image\/jpeg|image\/webp)$/.test(file.mimeType);
  const editing = !!editable && mode === 'edit';
  const imageEditing = file?.kind === 'image' && editing;
  const title = remoteFileName(browser.canonicalPath);
  const canTransfer = props.available && browser.transfer.supported && !!(fileOpen ? browser.target : browser.uploadFolder);
  const canCopy = !!browser.target && !browser.busy && !browser.clipboard.copying;
  const showPath = () => { Keyboard.dismiss(); browser.setPath(browser.canonicalPath); setPathOpen(true); };
  const showMenu = () => { Keyboard.dismiss(); setMenuOpen(true); };
  const copyPath = () => { browser.clearError(); void browser.clipboard.copy(); };
  const download = () => { if (browser.target && !blocked) browser.transfer.download(browser.target); };
  const upload = () => { if (browser.uploadFolder && !blocked) browser.transfer.upload(browser.uploadFolder); };
  props.backHandler.current = () => {
    if (menuOpen) { setMenuOpen(false); return; }
    if (pathOpen) { setPathOpen(false); return; }
    browser.back();
  };
  const items: FileMenuItem[] = [
    { label: fileOpen ? 'Show file path' : 'Change directory', action: showPath, disabled: blocked },
    ...(fileOpen ? [{ label: 'Copy file path', action: copyPath, disabled: !canCopy }] : []),
    ...(fileOpen && editable ? [{ label: mode === 'edit' ? file?.kind === 'image' ? 'Preview image' : 'Read file' : text ? 'Edit source' : 'Edit image',
      action: () => browser.changeMode(mode === 'edit' ? 'read' : 'edit'), disabled: blocked }] : []),
    ...(text && textPreviewKind(file.path, file.mimeType) !== 'code' ? [{ label: 'View code', action: () => browser.changeMode('code'), disabled: blocked }] : []),
    ...(text ? [{ label: browser.wrapLines ? 'Unwrap lines' : 'Wrap lines', action: () => browser.setWrapLines(!browser.wrapLines) }] : []),
    ...(fileOpen ? [{ label: 'Download selected file', action: download, disabled: !canTransfer || blocked }] : []),
    { label: fileOpen ? 'Reload' : 'Refresh', action: browser.reload, disabled: blocked || !props.available },
    ...(fileOpen || directory?.parentPath ? [{ label: 'Open parent folder', action: browser.parentFolder, disabled: blocked || !props.available }] : []),
    { label: 'Host home', action: () => browser.go('~'), disabled: blocked || !props.available },
  ];
  const notification = browser.clipboard.copied ? 'Path copied.' : browser.error ?? browser.transfer.error ?? browser.transfer.message
    ?? (!props.available && !offlineDismissed ? 'Host disconnected. Changes stay here until you close the file.' : null);
  const dismissNotice = () => { browser.clearError(); browser.transfer.dismiss(); browser.clipboard.clear(); setOfflineDismissed(true); };
  return <SafeAreaView testID="files-screen" style={styles.page} edges={['top', 'bottom', 'left', 'right']}>
    {!imageEditing && <FileToolbar title={title} file={fileOpen} editable={!!editable} editing={editing} text={text}
      dirty={!!browser.dirty} saving={browser.saving} blocked={blocked} canTransfer={canTransfer} canCopy={canCopy} canSave={props.available}
      close={browser.back} toggleMode={() => browser.changeMode(mode === 'edit' ? 'read' : 'edit')} path={showPath} copy={copyPath}
      download={download} upload={upload} refresh={browser.reload} more={showMenu} save={() => { void browser.save(); }} />}
    {fileOpen ? file && !browser.busy ? <FileContent key={file.path} file={file} draft={browser.draft} setDraft={browser.setDraft}
      saving={browser.saving || browser.transfer.running} mode={mode === 'edit' && !editable ? 'read' : mode} wrapLines={browser.wrapLines} openRequest={browser.initialPosition}
      previewHtml={browser.preview.html} previewLoading={browser.preview.loading} onPreviewError={browser.report}
      imageEditor={{ draftScope: browser.imageScope, onSave: browser.saveImage, onDirtyChange: browser.imageDirty,
        onSaved: browser.imageSaved, onFileAction: action => { if (action === 'close') browser.closeFile(); else showMenu(); } }} />
      : <View testID="files-content" collapsable={false} style={{ flex: 1, alignItems: 'center', justifyContent: 'center' }}>
        <Note>{browser.busy ? 'Loading…' : 'No preview available.'}</Note>
      </View>
      : <View testID="files-content" collapsable={false} style={styles.grow}>
        {directory ? <FlatList ref={list} testID="files-list" key={`${browser.context.sessionId ?? ''}/${browser.context.tabId ?? ''}/${directory.path}`}
          data={directory.entries} keyExtractor={entry => entry.path} style={styles.grow} keyboardShouldPersistTaps="handled"
          getItemLayout={(_data, index) => ({ length: 40, offset: index * 40, index })}
          onScroll={event => browser.trackScroll(event.nativeEvent.contentOffset.y)} scrollEventThrottle={16}
          onContentSizeChange={() => {
            if (browser.restoreOffset.current === null) return;
            const offset = browser.restoreOffset.current; browser.restoreOffset.current = null; browser.scrollOffset.current = offset;
            list.current?.scrollToOffset({ offset, animated: false });
          }}
          contentContainerStyle={!directory.entries.length ? { flexGrow: 1 } : undefined}
          ListEmptyComponent={<View style={{ flex: 1, alignItems: 'center', justifyContent: 'center' }}><Note>{browser.busy ? 'Loading…' : 'Empty folder'}</Note></View>}
          renderItem={({ item }) => <Pressable accessibilityRole="button" accessibilityLabel={`${item.kind === 'directory' ? 'Open folder' : 'Open file'} ${item.name}`}
            disabled={blocked || !props.available} onPress={() => browser.openEntry(item)}
            style={({ pressed }) => ({ width: '100%', height: 40, paddingHorizontal: 10, flexDirection: 'row', alignItems: 'center', gap: 8,
              borderBottomWidth: 0.5, borderColor: colors.line, backgroundColor: pressed ? colors.surface : colors.bg })}>
            <FileIconGlyph icon={item.kind === 'directory' ? 'folder' : 'document'} color={colors.muted} />
            <Text numberOfLines={1} style={{ flex: 1, minWidth: 0, color: colors.text, fontSize: 14 }}>{item.name}</Text>
            {item.kind === 'directory' && <View style={{ width: 12, alignItems: 'center', transform: [{ rotate: '180deg' }, { scale: 0.6 }] }}><FileIconGlyph icon="back" color={colors.muted} /></View>}
          </Pressable>} /> : <View style={{ flex: 1, alignItems: 'center', justifyContent: 'center' }}><Note>{browser.busy ? 'Loading…' : 'Folder unavailable.'}</Note></View>}
      </View>}
    <FileTransferBar progress={browser.transfer.progress} message={notification}
      error={!browser.clipboard.copied && !!(browser.error || browser.transfer.error || !props.available)} cancel={browser.transfer.cancel} dismiss={dismissNotice} />
    {pathOpen && <FilePathSheet api={props.directoryApi} value={browser.path} change={browser.setPath} hostName={props.hostName}
      editable={!blocked && props.available} close={() => setPathOpen(false)} submit={value => { setPathOpen(false); browser.go(value); }} />}
    {menuOpen && <FileMoreMenu path={browser.canonicalPath} hostName={props.hostName} items={items} close={() => setMenuOpen(false)} />}
  </SafeAreaView>;
}
