import React, { useEffect, useMemo, useState } from 'react';
import { Image, KeyboardAvoidingView, Platform, ScrollView, TextInput, View } from 'react-native';
import type { BrowserFile, FileOpenRequest } from './file-browser-types';
import { FileImageEditor, type FileImageEditorProps } from './FileImageEditor';
import { FilePreview } from './FilePreview';
import { sourceSelection, textPreviewKind } from './file-preview-document';
import { fileSourceLanguage } from './file-source-preview';
import { Button, Note, useUi } from './ui';
import { FileIconButton } from './FileIconButton';
import { sourceUnwrappedWidth } from './file-source-layout';

export function FileContent({ file, draft, setDraft, saving, available, save, reload, imageEditor, openRequest, previewHtml, previewLoading, onPreviewError }: {
  file: BrowserFile;
  draft: string;
  setDraft: (value: string) => void;
  saving: boolean;
  available: boolean;
  save: () => void;
  reload: () => void;
  imageEditor: Omit<FileImageEditorProps, 'image'>;
  openRequest?: FileOpenRequest;
  previewHtml?: string;
  previewLoading: boolean;
  onPreviewError: (message: string) => void;
}) {
  const { colors, styles } = useUi();
  const kind = textPreviewKind(file.path, file.mimeType);
  const [mode, setMode] = useState<'source' | 'preview' | 'code'>(() => openRequest?.line || kind === 'text' ? 'source' : kind === 'code' ? 'code' : 'preview');
  const [editingImage, setEditingImage] = useState(false);
  const [wrap, setWrap] = useState(true);
  const [sourceViewport, setSourceViewport] = useState(0);
  const sourceWidth = useMemo(() => sourceUnwrappedWidth(draft), [draft]);
  const [selection, setSelection] = useState<{ start: number; end: number } | undefined>(() =>
    file.kind === 'text' && openRequest?.line ? sourceSelection(file.text, openRequest.line, openRequest.column) : undefined);
  useEffect(() => { if (!selection) return; const timer = setTimeout(() => setSelection(undefined), 300); return () => clearTimeout(timer); }, [selection]);
  const imageSource = useMemo(() => file.kind === 'image' ? { uri: file.dataUrl } : undefined, [file]);
  if (file.kind === 'binary') return <View style={styles.content}><Note>No preview for this file type.</Note><Note>{file.size} bytes</Note></View>;
  if (file.kind === 'image') return (
    <View style={styles.grow}>
      <View style={[styles.row, { padding: 8 }]}>
        <FileIconButton icon="preview" label="Preview" active={!editingImage} disabled={saving} onPress={() => setEditingImage(false)} />
        {/^(?:image\/png|image\/jpeg|image\/webp)$/.test(file.mimeType) &&
          <FileIconButton icon="edit" label="Edit image" active={editingImage} disabled={saving} onPress={() => setEditingImage(true)} />}
        <View style={styles.grow} />
        <FileIconButton icon="reload" label="Reload" disabled={saving || !available} onPress={reload} />
      </View>
      {editingImage ? <FileImageEditor image={file} {...imageEditor} /> : /^(?:image\/x-icon|image\/vnd\.microsoft\.icon|image\/avif)$/.test(file.mimeType)
        ? <FilePreview text={file.dataUrl} kind="image" onError={onPreviewError} /> : <Image testID="file-image" accessibilityLabel={file.path} source={imageSource} resizeMode="contain" style={{ flex: 1, width: '100%' }}
        onError={() => onPreviewError('This image could not be displayed.')} />}
      <View style={{ padding: 8 }}><Note>{file.size} bytes</Note></View>
    </View>
  );
  const dirty = draft !== file.text;
  return (
    <KeyboardAvoidingView style={styles.grow} behavior={Platform.OS === 'ios' ? 'padding' : undefined}>
      <View style={[styles.row, { padding: 8, flexWrap: 'wrap', gap: 6 }]}>
        {kind !== 'text' && <>
          <Button title="Source" subtle={mode !== 'source'} onPress={() => setMode('source')} />
          {fileSourceLanguage(file.path) && <Button title="Code" subtle={mode !== 'code'} onPress={() => setMode('code')} />}
          {kind !== 'code' && <Button title="Preview" subtle={mode !== 'preview'} onPress={() => setMode('preview')} />} 
        </>}
        {(mode === 'source' || mode === 'code') && <Button title={wrap ? 'Unwrap' : 'Wrap'} subtle onPress={() => setWrap(value => !value)} />}
        <View style={styles.grow} />
        <Button title="Reload" subtle disabled={saving || !available} onPress={reload} />
        <Button title={saving ? 'Saving…' : 'Save'} disabled={!dirty || saving || !available} onPress={save} />
      </View>
      {dirty && <View style={{ paddingHorizontal: 8 }}><Note>Unsaved</Note></View>}
      {mode === 'source' ? (
        <ScrollView testID="source-scroll" horizontal scrollEnabled={!wrap} directionalLockEnabled style={styles.grow}
          onLayout={event => setSourceViewport(event.nativeEvent.layout.width)} keyboardShouldPersistTaps="handled" contentContainerStyle={{ flexGrow: 1 }}>
          <TextInput
            accessibilityLabel="File source"
            testID="file-source"
            value={draft}
            onChangeText={setDraft}
            selection={selection}
            multiline
            autoCapitalize="none"
            autoCorrect={false}
            spellCheck={false}
            editable={!saving}
            textAlignVertical="top"
            scrollEnabled
            style={{ width: sourceViewport ? wrap ? sourceViewport : Math.max(sourceViewport, sourceWidth) : wrap ? '100%' : sourceWidth,
              minHeight: 240, padding: 12, color: colors.text, backgroundColor: colors.bg,
              fontFamily: Platform.OS === 'ios' ? 'Menlo' : 'monospace', fontSize: 13 }}
          />
        </ScrollView>
      ) : kind !== 'text' ? (
        <>
          {kind === 'html' && mode === 'preview' && <View style={{ paddingHorizontal: 8 }}><Note>Isolated preview. Network and device access are blocked.</Note></View>}
          {kind === 'html' && mode === 'preview' && previewLoading ? <View style={styles.content}><Note>Loading preview…</Note></View>
            : <FilePreview text={kind === 'html' && mode === 'preview' ? previewHtml ?? '' : draft} kind={mode === 'code' ? 'code' : kind} path={file.path}
                preparedMarkdown={kind === 'markdown' ? previewHtml : undefined} wrapLines={wrap} onError={onPreviewError} />}
        </>
      ) : null}
    </KeyboardAvoidingView>
  );
}
