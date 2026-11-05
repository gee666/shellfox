import React, { useCallback, useEffect, useRef, useState } from 'react';
import { Alert, FlatList, Modal, Pressable, Text, TextInput, View } from 'react-native';
import { SafeAreaView } from 'react-native-safe-area-context';
import { prepareFileHtml } from '../../src/shared/file-html-preview';
import { failure, type ManagerApi } from '../../src/shared/contracts';
import { DirectoryPathInput, type DirectoryPathInputHandle } from './DirectoryPathInput';
import { CopyFilePathButton, FileTransferBar, FolderIcon } from './FileTransferBar';
import Clipboard from '@react-native-clipboard/clipboard';
import { useFileTransfers } from './useFileTransfers';
import { discardImageEditorDraft } from './FileImageEditor';
import { getImageDraftScope } from '../../src/shared/image-editor-drafts';
import { FileContent } from './FileContent';
import type { BrowserDirectory, BrowserFile, FileApi, FileOpenRequest } from './file-browser-types';
import type { FileLocation } from '../../src/shared/file-contracts';
import { fileParentPath, renderFileMarkdown, textPreviewKind } from './file-preview-document';
import { Button, Note, useUi } from './ui';

type Location = { kind: 'directory'; value: BrowserDirectory } | { kind: 'file'; value: BrowserFile };
export function FilesScreen({ api, sessionId, tabId, openRequest, available, directoryApi, connectionEpoch, closeLabel = '‹ Terminal', onClose }: {
  api: FileApi;
  sessionId?: string;
  directoryApi?: Pick<ManagerApi, 'getHomeDirectory' | 'completeDirectory'>;
  connectionEpoch?: string | null;
  closeLabel?: string;
  tabId?: string;
  openRequest?: FileOpenRequest;
  available: boolean;
  onClose: () => void;
}) {
  const { colors, styles } = useUi();
  const [location, setLocation] = useState<Location | null>(null);
  const [context, setContext] = useState<Omit<FileLocation, 'path'>>(() => sessionId ? { sessionId, ...(tabId ? { tabId } : {}) } : {});
  const [history, setHistory] = useState<string[]>([]);
  const initialPath = openRequest?.path ?? (sessionId ? '.' : '~');
  const [path, setPath] = useState(initialPath);
  const [downloadTarget, setDownloadTarget] = useState<string | null>(openRequest?.path ?? null);
  const pathInput = useRef<DirectoryPathInputHandle>(null);
  const [draft, setDraft] = useState('');
  const [imageDirty, setImageDirty] = useState(false);
  const [busy, setBusy] = useState(false);
  const [copying, setCopying] = useState(false);
  const [copied, setCopied] = useState(false);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [previewHtml, setPreviewHtml] = useState<string>();
  const [previewLoading, setPreviewLoading] = useState(false);
  const operation = useRef(0);
  const initialFilePath = useRef<string | null>(null);
  const writePending = useRef(false);
  const mounted = useRef(true);
  const file = location?.kind === 'file' ? location.value : null;
  const directory = location?.kind === 'directory' ? location.value : null;
  const transfer = useFileTransfers({ api, context, available, connectionEpoch,
    uploaded: () => { if (directory) void load(directory.path, 'directory', history); },
  });
  const uploadFolder = directory?.path ?? (file ? fileParentPath(file.path) : downloadTarget ? fileParentPath(downloadTarget) : null);
  const imageDraftScope = getImageDraftScope(api, context.sessionId, context.tabId);
  const dirty = imageDirty || location?.kind === 'file' && location.value.kind === 'text' && draft !== location.value.text;
  const confirmDiscard = (action: () => void) => {
    if (writePending.current) return;
    if (!dirty) { action(); return; }
    Alert.alert('Discard changes?', 'This file has unsaved changes.', [
      { text: 'Keep editing', style: 'cancel' }, { text: 'Discard', style: 'destructive', onPress: () => {
        if (location?.kind === 'file' && location.value.kind === 'image') discardImageEditorDraft(location.value, imageDraftScope);
        setImageDirty(false); action();
      } },
    ]);
  };
  const load = async (nextPath: string, kind: 'directory' | 'auto', nextHistory: string[]) => {
    if (writePending.current || !available) return;
    const id = ++operation.current;
    setCopied(false);
    const hostHome = nextPath === '' || nextPath === '~';
    const nextContext = hostHome ? {} : context;
    if (hostHome) { nextPath = '~'; kind = 'directory'; nextHistory = []; }
    if (kind === 'auto') { setDownloadTarget(nextPath); setPath(nextPath); }
    pathInput.current?.dismissSuggestions();
    setBusy(true); setError(null);
    try {
      let next: Location;
      if (kind === 'directory') {
        const result = await api.list({ ...nextContext, path: nextPath });
        if (!result.ok) throw new Error(result.error.message);
        next = { kind: 'directory', value: result.value };
      } else {
        const result = await api.read({ ...nextContext, path: nextPath });
        if (id !== operation.current) return;
        if (result.ok) next = { kind: 'file', value: result.value };
        else {
          const directory = await api.list({ ...nextContext, path: nextPath });
          if (!directory.ok) throw new Error(result.error.message);
          next = { kind: 'directory', value: directory.value };
        }
      }
      if (id !== operation.current) return;
      if (id === 1 && next.kind === 'file') initialFilePath.current = next.value.path;
      setContext(nextContext); setLocation(next); setHistory(nextHistory); setPath(next.value.path); setImageDirty(false);
      setDownloadTarget(next.kind === 'file' ? next.value.path : null);
      setDraft(next.kind === 'file' && next.value.kind === 'text' ? next.value.text : '');
    } catch (cause) {
      if (id === operation.current) setError(cause instanceof Error ? cause.message : 'Unable to open this path.');
    } finally { if (id === operation.current) setBusy(false); }
  };
  useEffect(() => {
    mounted.current = true;
    void load(initialPath, openRequest ? 'auto' : 'directory', []);
    return () => { mounted.current = false; operation.current++; };
  }, []);

  useEffect(() => {
    const kind = file?.kind === 'text' ? textPreviewKind(file.path, file.mimeType) : null;
    if (file?.kind !== 'text' || kind !== 'html' && kind !== 'markdown') {
      setPreviewHtml(undefined); setPreviewLoading(false); return;
    }
    let current = true;
    setPreviewLoading(true); setPreviewHtml(undefined);
    const timer = setTimeout(() => {
      const html = kind === 'markdown' ? renderFileMarkdown(draft) : draft;
      void prepareFileHtml(html, async assetPath => {
        if (!current) throw new Error('Preview changed.');
        if (!available) return null;
        const result = await api.readAsset({ ...context, path: file.path, assetPath });
        return result.ok ? result.value.dataUrl : null;
      }).then(html => {
        if (current) setPreviewHtml(html);
      }).catch(cause => {
        if (current) setError(cause instanceof Error ? cause.message : 'Unable to prepare file preview.');
      }).finally(() => { if (current) setPreviewLoading(false); });
    }, 250);
    return () => { current = false; clearTimeout(timer); };
  }, [api, context.sessionId, context.tabId, file?.path, file?.revision, draft, available]);

  useEffect(() => {
    if (!copied) return;
    const timer = setTimeout(() => setCopied(false), 1800);
    return () => clearTimeout(timer);
  }, [copied]);
  const copyPath = async () => {
    if (copying || !downloadTarget) return;
    const version = operation.current;
    setCopying(true); setCopied(false); setError(null);
    try {
      let canonical = file?.path;
      if (!canonical && api.beginRead && api.cancelTransfer) {
        const result = await api.beginRead({ ...context, path: downloadTarget });
        if (!result.ok) throw new Error(result.error.message);
        try { canonical = result.value.path; }
        finally { await api.cancelTransfer({ transferId: result.value.transferId }).catch(() => {}); }
      }
      if (!mounted.current || version !== operation.current) return;
      if (!canonical) throw new Error('Open the file before copying its path.');
      await Clipboard.setString(canonical);
      if (mounted.current && version === operation.current) setCopied(true);
    } catch (cause) {
      if (mounted.current && version === operation.current) setError(cause instanceof Error ? cause.message : 'Could not copy the file path.');
    } finally { if (mounted.current) setCopying(false); }
  };
  const save = async () => {
    if (!file || file.kind !== 'text' || writePending.current || !available) return;
    writePending.current = true; setSaving(true); setError(null);
    try {
      const result = await api.write({ ...context, path: file.path, text: draft, revision: file.revision });
      if (!mounted.current) return;
      if (!result.ok) throw new Error(result.error.message);
      setLocation({ kind: 'file', value: { ...file, text: draft, revision: result.value.revision, size: result.value.size } });
    } catch (cause) {
      if (mounted.current) setError(cause instanceof Error ? cause.message : 'Unable to save this file.');
    } finally { writePending.current = false; if (mounted.current) setSaving(false); }
  };
  const saveImage = async (dataUrl: string): ReturnType<FileApi['writeImage']> => {
    if (!file || file.kind !== 'image') return failure('VALIDATION', 'No image is open.');
    if (writePending.current || !available) return failure('NATIVE_UNAVAILABLE', 'Wait for the host connection before saving.');
    writePending.current = true; setSaving(true);
    try { return await api.writeImage({ ...context, path: file.path, revision: file.revision, dataUrl }); }
    catch (cause) { return failure('NATIVE_UNAVAILABLE', cause instanceof Error ? cause.message : 'Unable to save this image.'); }
    finally { writePending.current = false; if (mounted.current) setSaving(false); }
  };
  const back = () => {
    if (transfer.running) { transfer.cancel(); return; }
    confirmDiscard(() => {
    const previous = history.at(-1);
    if (previous) void load(previous, 'directory', history.slice(0, -1));
    else if (location?.kind === 'file') void load(fileParentPath(location.value.path), 'directory', []);
    else onClose();
    });
  };
  const close = () => confirmDiscard(onClose);
  const reportPreviewError = useCallback((message: string) => setError(message), []);
  const submitPath = (value: string) => confirmDiscard(() => { void load(value, 'auto', directory ? [...history, directory.path] : history); });
  return (
    <Modal visible animationType="slide" presentationStyle="fullScreen" onRequestClose={() => { if (!pathInput.current?.dismissSuggestions()) back(); }}>
      <SafeAreaView testID="files-screen" style={styles.page} edges={['top', 'bottom', 'left', 'right']}>
        <View style={styles.header}>
          <Button title={closeLabel} subtle onPress={close} disabled={saving || transfer.running} />
          <View accessible accessibilityRole="image" accessibilityLabel="Files" style={styles.grow}><FolderIcon /></View>
          {location?.kind === 'file' && <Button title="‹ Files" subtle disabled={busy || saving || transfer.running || !available} onPress={back} />}
        </View>
        <View style={[styles.row, { padding: 8 }]}>
          <View style={styles.grow}>{directoryApi ? <DirectoryPathInput ref={pathInput} api={directoryApi} label="File path" value={path}
            onChangeText={setPath} onSubmit={submitPath} onClose={back} editable={!saving && !busy && !transfer.running} />
            : <TextInput accessibilityLabel="File path" value={path} onChangeText={setPath} autoCapitalize="none" autoCorrect={false}
              editable={!saving && !busy && !transfer.running} style={styles.input} returnKeyType="go" onSubmitEditing={() => submitPath(path)} />}</View>
          <CopyFilePathButton disabled={busy || copying || !downloadTarget} onPress={() => { void copyPath(); }} />
          <Button title="Go" disabled={busy || saving || transfer.running || !available} onPress={() => submitPath(path)} />
        </View>
        {copied && <View style={{ paddingHorizontal: 8, paddingBottom: 4 }}><Note>Path copied.</Note></View>}
        <FileTransferBar canDownload={available && !busy && !saving && !!downloadTarget && transfer.supported}
          canUpload={available && !busy && !saving && !!uploadFolder && transfer.supported}
          download={() => { if (downloadTarget) transfer.download(downloadTarget); }} upload={() => { if (uploadFolder) transfer.upload(uploadFolder); }}
          progress={transfer.progress} message={transfer.message} error={transfer.error} cancel={transfer.cancel} dismiss={transfer.dismiss} />
        {!available && <View style={{ padding: 8 }}><Note error>Host disconnected. Changes are kept here until you close Files.</Note></View>}
        {error && <View style={[styles.row, { padding: 8 }]}><View style={styles.grow}><Note error>{error}</Note></View>
          <Button title="Dismiss" subtle onPress={() => setError(null)} /></View>}
        {busy && <View style={{ padding: 8 }}><Note>Loading…</Note></View>}
        {directory && <>
          <View style={[styles.row, { paddingHorizontal: 8, paddingBottom: 8 }]}>
            <Button title="Up" subtle disabled={busy || transfer.running || !available || !directory.parentPath}
              onPress={() => { if (directory.parentPath) void load(directory.parentPath, 'directory', [...history, directory.path]); }} />
            <Button title="Refresh" subtle disabled={busy || transfer.running || !available} onPress={() => { void load(directory.path, 'directory', history); }} />
            <View style={styles.grow} />
            <Note>{directory.entries.length} entries{directory.truncated ? ' · truncated' : ''}</Note>
          </View>
          <FlatList data={directory.entries} keyExtractor={entry => entry.path} style={styles.grow}
            ListEmptyComponent={<View style={styles.content}><Note>Empty folder</Note></View>}
            renderItem={({ item }) => (
              <Pressable accessibilityRole="button" accessibilityLabel={`${item.kind === 'directory' ? 'Open folder' : 'Open file'} ${item.name}`}
                disabled={busy || transfer.running || !available} onPress={() => { void load(item.path, item.kind === 'directory' ? 'directory' : 'auto', [...history, directory.path]); }}
                style={({ pressed }) => ({ paddingHorizontal: 12, minHeight: 44, justifyContent: 'center', borderBottomWidth: 1,
                  borderColor: colors.line, backgroundColor: pressed ? colors.panel : colors.bg, opacity: busy ? 0.5 : 1 })}>
                <Text numberOfLines={1} style={{ color: colors.text, fontFamily: 'monospace', fontSize: 14 }}>
                  {item.kind === 'directory' ? '› ' : item.kind === 'symlink' ? ' -> ' : '    '}{item.name}
                </Text>
              </Pressable>
            )} />
        </>}
        {file && !busy && <FileContent key={file.path} file={file} draft={draft} setDraft={setDraft} saving={saving || transfer.running} available={available}
          imageEditor={{ draftScope: imageDraftScope, onSave: saveImage, onDirtyChange: setImageDirty, onSaved: (result, dataUrl) => {
            if (file.kind !== 'image' || !mounted.current) return;
            setLocation({ kind: 'file', value: { ...file, revision: result.revision, size: result.size, dataUrl } });
            setImageDirty(false);
          } }}
          save={() => { void save(); }} reload={() => confirmDiscard(() => { void load(file.path, 'auto', history); })} openRequest={file.path === initialFilePath.current ? openRequest : undefined} previewHtml={previewHtml} previewLoading={previewLoading} onPreviewError={reportPreviewError} />}
        {!location && !busy && <View style={styles.content}><Button title="Retry" disabled={!available} onPress={() => { void load(path, 'auto', []); }} /></View>}
      </SafeAreaView>
    </Modal>
  );
}
