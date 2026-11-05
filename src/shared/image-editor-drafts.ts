import type { EditableImage, ImageEditorOperation } from './image-editor-protocol';

// API identity separates hosts, even when their paths, revisions and bytes match.
// Source bytes also keep an externally replaced image separate from its old draft.
interface Draft { image: EditableImage; scope: object; operations: ImageEditorOperation[]; users: number }
const defaultScope = {};
const contextScopes = new WeakMap<object, Map<string, object>>();
/** Stable across mounts. Session/tab context also separates identical paths in WSL distros. */
export function getImageDraftScope(api: object, sessionId?: string, tabId?: string): object {
  let contexts = contextScopes.get(api);
  if (!contexts) { contexts = new Map(); contextScopes.set(api, contexts); }
  const key = JSON.stringify([sessionId ?? null, tabId ?? null]);
  let scope = contexts.get(key);
  if (!scope) { scope = {}; contexts.set(key, scope); }
  return scope;
}
const drafts: Draft[] = [];
const listeners = new Set<() => void>();
const MAX_DRAFTS = 8;
function find(image: EditableImage, scope: object = defaultScope) {
  return drafts.find(draft => draft.scope === scope && draft.image.path === image.path && draft.image.revision === image.revision
    && draft.image.mimeType === image.mimeType && draft.image.dataUrl === image.dataUrl);
}
function changed(): void { listeners.forEach(listener => listener()); }
export function hasDirtyImageEditorDrafts(): boolean {
  return drafts.some(draft => draft.operations.length > 0);
}
export function subscribeImageEditorDrafts(listener: () => void): () => void {
  listeners.add(listener);
  return () => { listeners.delete(listener); };
}
export function acquireImageEditorDraft(image: EditableImage, scope: object = defaultScope): ImageEditorOperation[] {
  let draft = find(image, scope);
  if (!draft) {
    if (drafts.length >= MAX_DRAFTS) throw new Error('Eight image drafts are open. Save or discard one before editing another image.');
    draft = { image, scope, operations: [], users: 0 };
    drafts.push(draft);
  }
  draft.users++;
  return [...draft.operations];
}
export function releaseImageEditorDraft(image: EditableImage, scope: object = defaultScope): void {
  const draft = find(image, scope);
  if (!draft) return;
  draft.users = Math.max(0, draft.users - 1);
  if (!draft.users && !draft.operations.length) discardImageEditorDraft(image, scope);
}
export function storeImageEditorDraft(image: EditableImage, operations: ImageEditorOperation[], scope: object = defaultScope): void {
  const draft = find(image, scope);
  if (!draft) throw new Error('This image draft has been discarded. Reopen the image to edit it.');
  draft.operations = [...operations];
  changed();
}
/** A save may finish after unmount. Never erase edits made by a newer editor. */
export function completeImageEditorSave(image: EditableImage, operations: ImageEditorOperation[], scope: object = defaultScope): void {
  const draft = find(image, scope);
  if (!draft || JSON.stringify(draft.operations) !== JSON.stringify(operations)) return;
  draft.operations = [];
  if (!draft.users) discardImageEditorDraft(image, scope);
  else changed();
}
/** Call only after the user explicitly discards changes, or after a successful save. */
export function discardImageEditorDraft(image: EditableImage, scope: object = defaultScope): void {
  const draft = find(image, scope);
  if (draft) { drafts.splice(drafts.indexOf(draft), 1); changed(); }
}
