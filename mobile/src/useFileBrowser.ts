import { useCallback, useEffect, useRef, useState } from 'react';
import { Alert, Keyboard } from 'react-native';
import { failure } from '../../src/shared/contracts';
import type { FileLocation, FileWriteResult } from '../../src/shared/file-contracts';
import { discardImageEditorDraft, getImageDraftScope } from '../../src/shared/image-editor-drafts';
import type { BrowserDirectory, BrowserFile, FileApi, FileOpenRequest, FileViewMode } from './file-browser-types';
import { fileParentPath } from './file-preview-document';
import { useFileTransfers } from './useFileTransfers';
import { useFileHtmlPreview } from './useFileHtmlPreview';
import { useFilePathCopy } from './useFilePathCopy';

type Context = Omit<FileLocation, 'path'>;
export interface DirectorySnapshot { directory: BrowserDirectory; context: Context; offset: number }
interface Origin { page: DirectorySnapshot; history: DirectorySnapshot[] }
interface Options { api: FileApi; sessionId?: string; tabId?: string; openRequest?: FileOpenRequest; available: boolean; connectionEpoch?: string | null; onClose: () => void }
const sameScope = (a: Context, b: Context) => a.sessionId === b.sessionId && a.tabId === b.tabId;

/** Browser state survives temporary sheets; a file's origin is a page snapshot, not a fresh RPC. */
export function useFileBrowser({ api, sessionId, tabId, openRequest, available, connectionEpoch, onClose }: Options) {
  const initialPath = openRequest?.path ?? (sessionId ? '.' : '~');
  const [context, setContext] = useState<Context>(() => sessionId ? { sessionId, ...(tabId ? { tabId } : {}) } : {});
  const [directory, setDirectory] = useState<BrowserDirectory | null>(null);
  const [file, setFile] = useState<BrowserFile | null>(null);
  const [fileOpen, setFileOpen] = useState(!!openRequest);
  const [history, setHistory] = useState<DirectorySnapshot[]>([]);
  const [path, setPath] = useState(initialPath);
  const [target, setTarget] = useState<string | null>(openRequest?.path ?? null);
  const [draft, setDraft] = useState('');
  const [mode, setMode] = useState<FileViewMode>('read');
  const [wrapLines, setWrapLines] = useState(true);
  const [imageDirty, setImageDirty] = useState(false);
  const [busy, setBusy] = useState(false);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const origin = useRef<Origin | null>(null);
  const operation = useRef(0), mounted = useRef(true), writePending = useRef(false);
  const scrollOffset = useRef(0), restoreOffset = useRef<number | null>(null);
  const initialFile = useRef<{ path: string; context: Context } | null>(null);
  const renderVersion = operation.current;
  const report = useCallback((message: string) => setError(message), []);
  const preview = useFileHtmlPreview(api, context, file, draft, available, report);
  const clipboard = useFilePathCopy({ api, context, file, target, operation, mounted, report });
  const transfer = useFileTransfers({ api, context, available, connectionEpoch, uploaded: () => {
    if (operation.current === renderVersion && directory && !fileOpen) void load(directory.path, 'directory', history, true);
  } });
  const imageScope = getImageDraftScope(api, context.sessionId, context.tabId);
  const dirty = imageDirty || file?.kind === 'text' && draft !== file.text;
  const blocked = busy || saving || transfer.running;
  const uploadFolder = directory && !fileOpen ? directory.path : file ? fileParentPath(file.path) : target ? fileParentPath(target) : null;
  const canonicalPath = file?.path ?? (fileOpen ? target ?? path : directory?.path ?? path);
  const confirmDiscard = (action: () => void) => {
    if (writePending.current) return;
    if (!dirty) { action(); return; }
    Alert.alert('Discard changes?', 'This file has unsaved changes.', [
      { text: 'Keep editing', style: 'cancel' }, { text: 'Discard', style: 'destructive', onPress: () => {
        if (file?.kind === 'image') discardImageEditorDraft(file, imageScope);
        setImageDirty(false); action();
      } },
    ]);
  };
  const snapshot = (): DirectorySnapshot | null => directory ? { directory, context, offset: scrollOffset.current } : null;
  const navigationHistory = () => {
    const page = fileOpen ? origin.current?.page : snapshot();
    const previous = fileOpen ? origin.current?.history ?? history : history;
    return page ? [...previous, page].slice(-50) : previous;
  };
  async function load(nextPath: string, kind: 'directory' | 'auto', previous: DirectorySnapshot[], keepView = false) {
    if (writePending.current || !available) return;
    const id = ++operation.current;
    const hostHome = nextPath === '' || nextPath === '~', nextContext = hostHome ? {} : context;
    if (hostHome) { nextPath = '~'; kind = 'directory'; previous = []; }
    if (kind === 'auto') {
      const page = snapshot();
      if (!fileOpen && page) origin.current = { page, history };
      setFileOpen(true); setTarget(nextPath); setFile(null);
    }
    setPath(nextPath); setBusy(true); setError(null); clipboard.clear();
    try {
      let result: { kind: 'file'; value: BrowserFile } | { kind: 'directory'; value: BrowserDirectory };
      if (kind === 'directory') {
        const listed = await api.list({ ...nextContext, path: nextPath });
        if (!listed.ok) throw new Error(listed.error.message);
        result = { kind: 'directory', value: listed.value };
      } else {
        const read = await api.read({ ...nextContext, path: nextPath });
        if (id !== operation.current) return;
        if (read.ok) result = { kind: 'file', value: read.value };
        else {
          const listed = await api.list({ ...nextContext, path: nextPath });
          if (!listed.ok) throw new Error(read.error.message);
          result = { kind: 'directory', value: listed.value };
        }
      }
      if (id !== operation.current) return;
      setContext(nextContext); setPath(result.value.path); setImageDirty(false);
      if (result.kind === 'directory') {
        let offset = keepView ? scrollOffset.current : 0;
        const duplicate = previous.at(-1);
        if (duplicate?.directory.path === result.value.path && sameScope(duplicate.context, nextContext)) {
          offset = duplicate.offset; previous = previous.slice(0, -1);
        }
        origin.current = null; scrollOffset.current = offset; restoreOffset.current = offset;
        setDirectory(result.value); setHistory(previous); setFileOpen(false); setFile(null); setTarget(null); setDraft('');
      } else {
        if (id === 1) initialFile.current = { path: result.value.path, context: nextContext };
        setFile(result.value); setFileOpen(true); setTarget(result.value.path);
        setDraft(result.value.kind === 'text' ? result.value.text : '');
        if (!keepView) { setMode(id === 1 && openRequest?.line && result.value.kind === 'text' ? 'edit' : 'read'); setWrapLines(true); }
      }
    } catch (cause) {
      if (id === operation.current) report(cause instanceof Error ? cause.message : 'Unable to open this path.');
    } finally { if (id === operation.current) setBusy(false); }
  }
  useEffect(() => {
    mounted.current = true;
    void load(initialPath, openRequest ? 'auto' : 'directory', []);
    return () => { mounted.current = false; operation.current++; };
  }, []);
  const restore = (page: DirectorySnapshot, previous: DirectorySnapshot[]) => {
    operation.current++; origin.current = null;
    scrollOffset.current = page.offset; restoreOffset.current = page.offset;
    setContext(page.context); setDirectory(page.directory); setHistory(previous); setPath(page.directory.path);
    setFileOpen(false); setFile(null); setTarget(null); setDraft(''); setImageDirty(false); setBusy(false); setError(null); clipboard.clear();
  };
  const closeFile = () => {
    Keyboard.dismiss();
    confirmDiscard(() => {
      transfer.cancel();
      if (origin.current) restore(origin.current.page, origin.current.history);
      else { operation.current++; onClose(); }
    });
  };
  const back = () => {
    Keyboard.dismiss();
    if (fileOpen) { closeFile(); return; }
    transfer.cancel();
    const page = history.at(-1);
    if (page) restore(page, history.slice(0, -1)); else { operation.current++; onClose(); }
  };
  const go = (value: string) => {
    if (transfer.running) return;
    Keyboard.dismiss(); confirmDiscard(() => { void load(value, 'auto', navigationHistory()); });
  };
  const openEntry = (entry: BrowserDirectory['entries'][number]) => {
    if (blocked) return;
    Keyboard.dismiss(); void load(entry.path, entry.kind === 'directory' ? 'directory' : 'auto', navigationHistory());
  };
  const changeMode = (next: FileViewMode) => {
    if (blocked) return;
    Keyboard.dismiss();
    if (file?.kind === 'image' && mode === 'edit' && next !== 'edit') confirmDiscard(() => setMode(next));
    else setMode(next);
  };
  const reload = () => {
    Keyboard.dismiss();
    confirmDiscard(() => { void load(canonicalPath, fileOpen ? 'auto' : 'directory', history, true); });
  };
  const save = async () => {
    if (file?.kind !== 'text' || writePending.current || !available) return;
    writePending.current = true; setSaving(true); setError(null);
    try {
      const result = await api.write({ ...context, path: file.path, text: draft, revision: file.revision });
      if (!mounted.current) return;
      if (!result.ok) throw new Error(result.error.message);
      setFile({ ...file, text: draft, revision: result.value.revision, size: result.value.size });
    } catch (cause) { if (mounted.current) report(cause instanceof Error ? cause.message : 'Unable to save this file.'); }
    finally { writePending.current = false; if (mounted.current) setSaving(false); }
  };
  const saveImage = async (dataUrl: string): ReturnType<FileApi['writeImage']> => {
    if (file?.kind !== 'image') return failure('VALIDATION', 'No image is open.');
    if (writePending.current || !available) return failure('NATIVE_UNAVAILABLE', 'Wait for the host connection before saving.');
    writePending.current = true; setSaving(true);
    try { return await api.writeImage({ ...context, path: file.path, revision: file.revision, dataUrl }); }
    catch (cause) { return failure('NATIVE_UNAVAILABLE', cause instanceof Error ? cause.message : 'Unable to save this image.'); }
    finally { writePending.current = false; if (mounted.current) setSaving(false); }
  };
  const imageSaved = (result: FileWriteResult, dataUrl: string) => {
    if (file?.kind !== 'image' || !mounted.current) return;
    setFile({ ...file, revision: result.revision, size: result.size, dataUrl }); setImageDirty(false);
  };
  const parentFolder = () => {
    if (blocked) return;
    const parent = !fileOpen && directory ? directory.parentPath : fileParentPath(canonicalPath);
    if (parent) confirmDiscard(() => { void load(parent, 'directory', navigationHistory()); });
  };
  const initialPosition = file && initialFile.current?.path === file.path && sameScope(initialFile.current.context, context) ? openRequest : undefined;
  const updateDraft = (value: string) => { if (operation.current === renderVersion && fileOpen) setDraft(value); };
  const trackScroll = (offset: number) => { if (operation.current === renderVersion && !fileOpen) scrollOffset.current = offset; };
  return { file, directory, fileOpen, path, setPath, target, draft, setDraft: updateDraft, mode, wrapLines, setWrapLines, dirty, busy, saving, blocked,
    error, report, clearError: () => setError(null), context, canonicalPath, imageScope, imageDirty: setImageDirty, imageSaved, initialPosition,
    preview, clipboard, transfer, uploadFolder, scrollOffset, restoreOffset, trackScroll,
    back, closeFile, go, openEntry, changeMode, reload, save, saveImage, parentFolder,
  };
}
