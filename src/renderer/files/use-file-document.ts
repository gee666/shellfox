import { useEffect, useRef, useState } from 'react';
import type { ManagerApi } from '../../shared/contracts';
import type { FileList, FileWriteResult } from '../../shared/file-contracts';
import { openFileTarget } from '../../shared/file-actions';
import { getFileDraft, isFileSaving, markFileSaving, rememberFile, retainFileDraft, subscribeFileDraft, type FileDraft } from './file-drafts';

type FileApi = NonNullable<ManagerApi['files']>;
type ReadResult = Awaited<ReturnType<FileApi['read']>>;
type ReadFile = Extract<ReadResult, { ok: true }>['value'];
export interface FileOpenRequest { path: string; line?: number; column?: number; id: number | string; sessionId?: string; tabId?: string }
interface DocumentState {
  file: ReadFile | null;
  draft: FileDraft | null;
  request: FileOpenRequest | null;
  loading: boolean;
  saving: boolean;
  error: string | null;
  /** Absolute path the failed open searched, shown so users can check it. */
  errorPath: string | null;
}
const initialState: DocumentState = { file: null, draft: null, request: null, loading: false, saving: false, error: null, errorPath: null };
const message = (error: unknown) => error instanceof Error ? error.message : 'File operation failed.';

export function useFileDocument(api: ManagerApi, sessionId?: string, tabId?: string) {
  const [state, setState] = useState(initialState);
  const current = useRef(state);
  current.current = state;
  const generation = useRef(0);
  const saving = useRef(false);
  useEffect(() => {
    const unsubscribe = subscribeFileDraft(api, (session, tab, draft) => {
      if (session === sessionId && tab === tabId && current.current.draft?.path === draft.path) {
        const next = { ...current.current, draft, saving: isFileSaving(api, sessionId, tabId, draft.path) }; current.current = next; setState(next);
      }
    });
    return () => { generation.current++; unsubscribe(); };
  }, [api, sessionId, tabId]);
  const target = { ...(sessionId ? { sessionId } : {}), ...(tabId ? { tabId } : {}) };
  const update = (next: DocumentState) => { current.current = next; setState(next); };

  const fail = (request: FileOpenRequest, error: string, errorPath: string | null) => {
    const retained = getFileDraft(api, sessionId, tabId, request.path);
    if (retained) {
      const draft = { ...retained, conflict: true };
      const file: ReadFile = { kind: 'text', path: draft.path, text: draft.savedText, revision: draft.revision, mimeType: draft.mimeType, size: new TextEncoder().encode(draft.savedText).byteLength };
      update({ ...initialState, file, draft, request, error });
    } else update({ ...initialState, request, error, errorPath });
  };
  /** Resolves to the folder listing when the request names a directory. */
  const open = async (request: FileOpenRequest): Promise<FileList | null> => {
    const requestId = ++generation.current;
    update({ ...initialState, request, loading: true });
    try {
      if (!api.files) throw new Error('File browsing is not supported by this host.');
      const scope = request.sessionId ? { sessionId: request.sessionId, ...(request.tabId ? { tabId: request.tabId } : {}) } : target;
      const result = await openFileTarget(api.files, { ...scope, path: request.path });
      if (requestId !== generation.current) return null;
      if (result.kind === 'directory') { update(initialState); return result.listing; }
      if (result.kind === 'error') { fail(request, result.error.message, result.path); return null; }
      const file = result.file;
      let draft: FileDraft | null = null;
      if (file.kind === 'text') {
        const cached = getFileDraft(api, sessionId, tabId, file.path);
        draft = cached && cached.text !== cached.savedText
          ? { ...cached, conflict: cached.revision !== file.revision }
          : { path: file.path, text: file.text, savedText: file.text, revision: file.revision, mimeType: file.mimeType, conflict: false };
        retainFileDraft(api, sessionId, tabId, draft);
      }
      rememberFile(api, sessionId, tabId, file.path);
      update({ ...initialState, file, draft, request, saving: isFileSaving(api, sessionId, tabId, file.path) });
    } catch (error) { if (requestId === generation.current) fail(request, message(error), null); }
    return null;
  };

  const setText = (text: string) => {
    const previous = current.current;
    if (!previous.draft) return;
    const draft = { ...previous.draft, text };
    retainFileDraft(api, sessionId, tabId, draft);
    update({ ...previous, draft });
  };
  const discard = () => {
    const previous = current.current;
    if (!previous.draft) return;
    const draft = { ...previous.draft, text: previous.draft.savedText };
    retainFileDraft(api, sessionId, tabId, draft);
    update({ ...previous, draft, error: null });
  };
  const save = async (): Promise<boolean> => {
    const previous = current.current;
    if (!previous.draft || previous.draft.text === previous.draft.savedText) return true;
    if (saving.current || isFileSaving(api, sessionId, tabId, previous.draft.path)) return false;
    const written = previous.draft;
    const requestId = generation.current;
    saving.current = true;
    markFileSaving(api, sessionId, tabId, written.path, true);
    update({ ...previous, saving: true, error: null });
    try {
      if (!api.files) throw new Error('File editing is not supported by this host.');
      const result = await api.files.write({ ...target, path: written.path, text: written.text, revision: written.revision });
      if (!result.ok) throw new Error(result.error.message);
      const visible = current.current.draft;
      const latest = getFileDraft(api, sessionId, tabId, written.path) ?? (visible?.path === written.path ? visible : written);
      const draft = { ...latest, savedText: written.text, revision: result.value.revision, conflict: false };
      retainFileDraft(api, sessionId, tabId, draft);
      if (requestId === generation.current) update({ ...current.current, draft, saving: false });
      return draft.text === draft.savedText;
    } catch (error) {
      if (requestId === generation.current) update({ ...current.current, saving: false, error: message(error) });
      return false;
    } finally { saving.current = false; markFileSaving(api, sessionId, tabId, written.path, false); }
  };
  const reveal = (request: FileOpenRequest) => update({ ...current.current, request });
  const updateImage = (result: FileWriteResult, dataUrl: string) => {
    const previous = current.current;
    if (previous.file?.kind !== 'image' || previous.file.path !== result.path) return;
    update({ ...previous, file: { ...previous.file, revision: result.revision, size: result.size, dataUrl }, error: null });
  };
  return { ...state, dirty: state.draft !== null && state.draft.text !== state.draft.savedText, open, reveal, setText, discard, save, updateImage };
}
