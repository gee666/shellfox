import type { ManagerApi } from '../../shared/contracts';

export interface FileDraft {
  path: string;
  text: string;
  savedText: string;
  revision: string;
  mimeType: string;
  conflict: boolean;
}

const drafts = new WeakMap<ManagerApi, Map<string, FileDraft>>();
const unloadGuards = new WeakMap<ManagerApi, (event: BeforeUnloadEvent) => void>();
const lastFiles = new WeakMap<ManagerApi, Map<string, string>>();
const savingDrafts = new WeakMap<ManagerApi, Set<string>>();
type DraftListener = (sessionId: string | undefined, tabId: string | undefined, draft: FileDraft) => void;
const listeners = new WeakMap<ManagerApi, Set<DraftListener>>();
export function subscribeFileDraft(api: ManagerApi, listener: DraftListener): () => void {
  let subscriptions = listeners.get(api);
  if (!subscriptions) { subscriptions = new Set(); listeners.set(api, subscriptions); }
  subscriptions.add(listener);
  return () => { subscriptions.delete(listener); };
}
const scopeIds = new WeakMap<ManagerApi, number>();
let nextScopeId = 0;
export function fileApiScopeId(api: ManagerApi): number {
  const existing = scopeIds.get(api);
  if (existing !== undefined) return existing;
  const id = ++nextScopeId; scopeIds.set(api, id); return id;
}
export function rememberFile(api: ManagerApi, sessionId: string | undefined, tabId: string | undefined, path: string): void {
  let files = lastFiles.get(api);
  if (!files) { files = new Map(); lastFiles.set(api, files); }
  files.set(draftKey(sessionId, tabId, ''), path);
}
export function rememberedFile(api: ManagerApi, sessionId: string | undefined, tabId?: string): string | undefined {
  return lastFiles.get(api)?.get(draftKey(sessionId, tabId, ''));
}
function draftKey(sessionId: string | undefined, tabId: string | undefined, path: string): string {
  return JSON.stringify([sessionId, tabId ?? null, path]);
}
export function getFileDraft(api: ManagerApi, sessionId: string | undefined, tabId: string | undefined, path: string): FileDraft | undefined {
  return drafts.get(api)?.get(draftKey(sessionId, tabId, path));
}
export function isFileSaving(api: ManagerApi, sessionId: string | undefined, tabId: string | undefined, path: string): boolean {
  return savingDrafts.get(api)?.has(draftKey(sessionId, tabId, path)) ?? false;
}
export function markFileSaving(api: ManagerApi, sessionId: string | undefined, tabId: string | undefined, path: string, saving: boolean): void {
  let keys = savingDrafts.get(api);
  if (!keys) { keys = new Set(); savingDrafts.set(api, keys); }
  const key = draftKey(sessionId, tabId, path);
  if (saving) keys.add(key); else keys.delete(key);
  const draft = getFileDraft(api, sessionId, tabId, path);
  if (draft) retainFileDraft(api, sessionId, tabId, draft);
}
export function retainFileDraft(api: ManagerApi, sessionId: string | undefined, tabId: string | undefined, draft: FileDraft): void {
  let cache = drafts.get(api);
  if (!cache) { cache = new Map(); drafts.set(api, cache); }
  const key = draftKey(sessionId, tabId, draft.path);
  if (draft.text === draft.savedText && !savingDrafts.get(api)?.has(key)) cache.delete(key);
  else cache.set(key, draft);
  listeners.get(api)?.forEach(listener => listener(sessionId, tabId, draft));
  const existing = unloadGuards.get(api);
  const dirty = [...cache.values()].some(value => value.text !== value.savedText);
  if (dirty && !existing) {
    const guard = (event: BeforeUnloadEvent) => { event.preventDefault(); event.returnValue = ''; };
    window.addEventListener('beforeunload', guard);
    unloadGuards.set(api, guard);
  } else if (!dirty && existing) {
    window.removeEventListener('beforeunload', existing);
    unloadGuards.delete(api);
  }
}
