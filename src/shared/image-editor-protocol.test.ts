import { afterEach, describe, expect, it } from 'vitest';
import { FILE_IMAGE_WRITE_BYTES } from './file-schemas';
import {
  allowImageEditorNavigation, parseImageEditorMessage, validateEditorImage,
  type EditableImage,
} from './image-editor-protocol';
import { acquireImageEditorDraft, completeImageEditorSave, discardImageEditorDraft, getImageDraftScope, releaseImageEditorDraft, storeImageEditorDraft } from './image-editor-drafts';

const token = 'image_editor_test_token';
const image: EditableImage = { kind: 'image', path: '/photo.png', mimeType: 'image/png', revision: 'a'.repeat(64), size: 3, dataUrl: 'data:image/png;base64,AAAA' };
const images: EditableImage[] = [];
function draft(index: number) { const value = { ...image, path: `/photo-${index}.png` }; images.push(value); return value; }
afterEach(() => { images.forEach(value => discardImageEditorDraft(value)); discardImageEditorDraft(image); });

describe('image editor bridge', () => {
  it('accepts only the document token and narrow, strict message shapes', () => {
    const state = { type: 'state', token, dirty: true, width: 3, height: 2, operations: [{ type: 'rotate', direction: 'right' }] };
    expect(parseImageEditorMessage(state, token)).toEqual(state);
    expect(parseImageEditorMessage(JSON.stringify(state), token)).toEqual(state);
    for (const value of [{ ...state, token: 'other_editor_token' }, { ...state, path: '/etc/passwd' }, { ...state, width: 9000 },
      { ...state, operations: [{ type: 'run', code: 'attack()' }] }, { ...state, operations: new Array(257).fill({ type: 'flip', axis: 'horizontal' }) },
      { type: 'write', token, dataUrl: image.dataUrl }, '{']) expect(parseImageEditorMessage(value, token)).toBeNull();
  });
  it('accepts only known mobile file actions with the current token and no additional fields', () => {
    for (const action of ['close', 'more']) expect(parseImageEditorMessage({ type: 'file-action', action, token }, token)).toEqual({ type: 'file-action', action, token });
    for (const value of [{ type: 'file-action', action: 'copy', token }, { type: 'file-action', action: 'more', token: 'foreign_document_token' },
      { type: 'file-action', action: 'close', token, path: '/private' }, { type: 'file-action', action: 'more', token, dataUrl: image.dataUrl }]) {
      expect(parseImageEditorMessage(value, token)).toBeNull();
    }
  });
  it('accepts only a token-bound save request with no privileged or payload fields', () => {
    expect(parseImageEditorMessage({ type: 'save-request', token }, token)).toEqual({ type: 'save-request', token });
    for (const value of [{ type: 'save-request', token: 'foreign_document_token' }, { type: 'save-request', token, path: '/other.png' },
      { type: 'save-request', token, dataUrl: image.dataUrl }]) expect(parseImageEditorMessage(value, token)).toBeNull();
  });
  it('rejects arbitrary data types, invalid base64 and encoded payloads above the byte limit', () => {
    for (const dataUrl of ['data:image/svg+xml;base64,AAAA', 'data:text/html;base64,AAAA', 'data:image/png;base64,a', 'data:image/png;base64,',
      `data:image/png;base64,${Buffer.alloc(FILE_IMAGE_WRITE_BYTES + 1).toString('base64')}`]) {
      expect(parseImageEditorMessage({ type: 'export', token, dataUrl }, token)).toBeNull();
    }
    expect(parseImageEditorMessage({ type: 'export', token, dataUrl: image.dataUrl }, token)?.type).toBe('export');
  });
  it('rejects unsupported originals and mime mismatches before creating an editor document', () => {
    expect(() => validateEditorImage(image)).not.toThrow();
    expect(() => validateEditorImage({ ...image, mimeType: 'image/gif' })).toThrow();
    expect(() => validateEditorImage({ ...image, mimeType: 'image/jpeg' })).toThrow();
    expect(() => validateEditorImage({ ...image, dataUrl: 'https://example.com/image.png' })).toThrow();
  });
  it('allows no external, file, data, script or fragment navigation in the native wrapper', () => {
    expect(allowImageEditorNavigation('about:blank')).toBe(true);
    expect(allowImageEditorNavigation('about:srcdoc')).toBe(true);
    for (const url of ['https://example.com', 'file:///etc/passwd', 'data:text/html,x', 'javascript:attack()', 'about:blank#next', 'intent://open']) {
      expect(allowImageEditorNavigation(url)).toBe(false);
    }
  });
});

describe('bounded image drafts', () => {
  it('replays edits across unmount and keeps different hosts source bytes separate', () => {
    acquireImageEditorDraft(image);
    const operations = [{ type: 'rotate', direction: 'left' }] satisfies Parameters<typeof storeImageEditorDraft>[1];
    storeImageEditorDraft(image, operations); releaseImageEditorDraft(image);
    expect(acquireImageEditorDraft({ ...image })).toEqual(operations);
    const other = { ...image, dataUrl: 'data:image/png;base64,BBBB' }; images.push(other);
    expect(acquireImageEditorDraft(other)).toEqual([]);
    discardImageEditorDraft(image);
    expect(acquireImageEditorDraft(image)).toEqual([]);
  });
  it('keeps contextual API scopes stable across mounts and separates WSL sessions/tabs', () => {
    const api = {};
    expect(getImageDraftScope(api, 'session-a', 'tab-a')).toBe(getImageDraftScope(api, 'session-a', 'tab-a'));
    expect(getImageDraftScope(api)).toBe(getImageDraftScope(api));
    expect(getImageDraftScope(api, 'session-a', 'tab-a')).not.toBe(getImageDraftScope(api, 'session-b', 'tab-a'));
    expect(getImageDraftScope(api, 'session-a', 'tab-a')).not.toBe(getImageDraftScope(api, 'session-a', 'tab-b'));
    expect(getImageDraftScope(api)).not.toBe(getImageDraftScope({}));
  });
  it('isolates identical images by host scope through release, save and discard', () => {
    const hostA = {}, hostB = {};
    const first = [{ type: 'rotate', direction: 'left' }] satisfies Parameters<typeof storeImageEditorDraft>[1];
    const second = [{ type: 'flip', axis: 'vertical' }] satisfies Parameters<typeof storeImageEditorDraft>[1];
    try {
      acquireImageEditorDraft(image, hostA); storeImageEditorDraft(image, first, hostA); releaseImageEditorDraft(image, hostA);
      expect(acquireImageEditorDraft(image, hostB)).toEqual([]);
      storeImageEditorDraft(image, second, hostB); releaseImageEditorDraft(image, hostB);
      expect(acquireImageEditorDraft(image, hostA)).toEqual(first);
      expect(acquireImageEditorDraft(image, hostB)).toEqual(second);
      completeImageEditorSave(image, first, hostA);
      expect(acquireImageEditorDraft(image, hostA)).toEqual([]);
      discardImageEditorDraft(image, hostA);
      expect(acquireImageEditorDraft(image, hostB)).toEqual(second);
    } finally { discardImageEditorDraft(image, hostA); discardImageEditorDraft(image, hostB); }
  });
  it('cleans up saves completed after unmount without erasing newer edits', () => {
    const first = [{ type: 'rotate', direction: 'left' }] satisfies Parameters<typeof storeImageEditorDraft>[1];
    const newer = [{ type: 'flip', axis: 'horizontal' }] satisfies Parameters<typeof storeImageEditorDraft>[1];
    acquireImageEditorDraft(image); storeImageEditorDraft(image, first); releaseImageEditorDraft(image);
    acquireImageEditorDraft(image); storeImageEditorDraft(image, newer);
    completeImageEditorSave(image, first);
    expect(acquireImageEditorDraft(image)).toEqual(newer);
    releaseImageEditorDraft(image); releaseImageEditorDraft(image);
    completeImageEditorSave(image, newer);
    expect(acquireImageEditorDraft(image)).toEqual([]);
  });
  it('drops clean unmounted entries and refuses a ninth draft without evicting edits', () => {
    const clean = draft(100); acquireImageEditorDraft(clean); releaseImageEditorDraft(clean);
    const entries = Array.from({ length: 8 }, (_, index) => draft(index));
    for (const entry of entries) {
      acquireImageEditorDraft(entry); storeImageEditorDraft(entry, [{ type: 'flip', axis: 'vertical' }]); releaseImageEditorDraft(entry);
    }
    expect(() => acquireImageEditorDraft(draft(9))).toThrow('Eight image drafts');
    expect(acquireImageEditorDraft(entries[0])).toEqual([{ type: 'flip', axis: 'vertical' }]);
    discardImageEditorDraft(entries[0]);
    expect(acquireImageEditorDraft(draft(10))).toEqual([]);
  });
});
