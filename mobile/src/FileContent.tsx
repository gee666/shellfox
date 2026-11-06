import React, { useEffect, useLayoutEffect, useMemo, useRef, useState, type ComponentRef } from 'react';
import { Image, KeyboardAvoidingView, Platform, ScrollView, TextInput, View } from 'react-native';
import type { BrowserFile, FileOpenRequest, FileViewMode } from './file-browser-types';
import { FileImageEditor, type FileImageEditorProps } from './FileImageEditor';
import { FilePreview } from './FilePreview';
import { sourceSelection, textPreviewKind } from './file-preview-document';
import { sourceUnwrappedWidth } from './file-source-layout';
import { Note, useUi } from './ui';

/** File contents only. Navigation and actions belong to the single screen toolbar. */
export function FileContent({ file, draft, setDraft, saving, mode, wrapLines, imageEditor, openRequest, previewHtml, previewLoading, onPreviewError }: {
  file: BrowserFile;
  draft: string;
  setDraft: (value: string) => void;
  saving: boolean;
  mode: FileViewMode;
  wrapLines: boolean;
  imageEditor: Omit<FileImageEditorProps, 'image'>;
  openRequest?: FileOpenRequest;
  previewHtml?: string;
  previewLoading: boolean;
  onPreviewError: (message: string) => void;
}) {
  const { colors, styles } = useUi();
  const [sourceViewport, setSourceViewport] = useState(0);
  const sourceWidth = useMemo(() => sourceUnwrappedWidth(draft), [draft]);
  const input = useRef<ComponentRef<typeof TextInput>>(null);
  const [selection, setSelection] = useState<{ start: number; end: number } | undefined>(() =>
    file.kind === 'text' && openRequest?.line ? sourceSelection(file.text, openRequest.line, openRequest.column) : undefined);
  const caret = useRef(selection), previousMode = useRef(mode), currentMode = useRef(mode);
  currentMode.current = mode;
  const restoring = useRef(selection);
  const releaseSelection = () => { restoring.current = undefined; setSelection(undefined); };
  useEffect(() => {
    if (!selection) return;
    const pending = selection;
    const timer = setTimeout(() => {
      if (restoring.current === pending) restoring.current = undefined;
      setSelection(current => current === pending ? undefined : current);
    }, 300);
    return () => clearTimeout(timer);
  }, [selection]);
  useLayoutEffect(() => {
    const previous = previousMode.current; previousMode.current = mode;
    if (mode !== 'edit') { releaseSelection(); return; }
    if (previous === 'edit' || file.kind !== 'text') return;
    const range = caret.current ? { start: Math.min(caret.current.start, draft.length), end: Math.min(caret.current.end, draft.length) } : undefined;
    // RN synchronizes controlled text and selection atomically after focus.
    // Keep the range until its native ACK, never as a permanent cursor pin.
    restoring.current = range;
    if (range) caret.current = range;
    setSelection(range);
    input.current?.focus();
  }, [mode]);
  const imageSource = useMemo(() => file.kind === 'image' ? { uri: file.dataUrl } : undefined, [file]);
  const kind = textPreviewKind(file.path, file.mimeType);
  const previewKind = mode === 'code' || kind === 'text' ? 'code' : kind;
  return <KeyboardAvoidingView testID="files-content" collapsable={false} style={styles.grow} behavior={Platform.OS === 'ios' ? 'padding' : undefined}>
    {file.kind === 'binary' ? <View style={{ flex: 1, alignItems: 'center', justifyContent: 'center' }}><Note>No preview available.</Note></View>
      : file.kind === 'image' ? mode === 'edit' ? <FileImageEditor image={file} {...imageEditor} />
        : /^(?:image\/x-icon|image\/vnd\.microsoft\.icon|image\/avif)$/.test(file.mimeType)
          ? <FilePreview text={file.dataUrl} kind="image" onError={onPreviewError} />
          : <Image testID="file-image" accessibilityLabel={file.path} source={imageSource} resizeMode="contain" style={{ flex: 1, width: '100%' }}
            onError={() => onPreviewError('This image could not be displayed.')} />
        : mode === 'edit' ? <ScrollView testID="source-scroll" horizontal scrollEnabled={!wrapLines} directionalLockEnabled style={styles.grow}
          onLayout={event => setSourceViewport(event.nativeEvent.layout.width)} keyboardShouldPersistTaps="handled" contentContainerStyle={{ flexGrow: 1 }}>
          <TextInput ref={input} accessibilityLabel="File source" testID="file-source" value={draft} selection={selection}
            onChangeText={value => { releaseSelection(); setDraft(value); }} onPressIn={releaseSelection}
            onSelectionChange={event => {
              const range = event.nativeEvent.selection, expected = restoring.current;
              if (currentMode.current !== 'edit' || !Number.isInteger(range.start) || !Number.isInteger(range.end) || range.start < 0 || range.end < range.start) return;
              if (expected && (range.start !== expected.start || range.end !== expected.end)) return;
              caret.current = range;
              if (expected) releaseSelection();
            }}
            multiline disableFullscreenUI autoCapitalize="none" autoCorrect={false} spellCheck={false} editable={!saving} textAlignVertical="top" scrollEnabled
            style={{ width: sourceViewport ? wrapLines ? sourceViewport : Math.max(sourceViewport, sourceWidth) : wrapLines ? '100%' : sourceWidth,
              minHeight: 120, padding: 10, color: colors.text, backgroundColor: colors.bg,
              fontFamily: Platform.OS === 'ios' ? 'Menlo' : 'monospace', fontSize: 13 }} />
        </ScrollView>
          : previewKind === 'html' && previewLoading ? <View style={{ flex: 1, alignItems: 'center', justifyContent: 'center' }}><Note>Loading preview…</Note></View>
            : <FilePreview text={previewKind === 'html' ? previewHtml ?? '' : draft} kind={previewKind} path={file.path}
              preparedMarkdown={previewKind === 'markdown' ? previewHtml : undefined} wrapLines={wrapLines} onError={onPreviewError} />}
  </KeyboardAvoidingView>;
}
