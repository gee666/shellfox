// @vitest-environment jsdom
import '@testing-library/jest-dom/vitest';
import { cleanup, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { success, type ManagerApi } from '../../shared/contracts';
import type { FileApi, FileRead } from '../../shared/file-contracts';
import type { FileImageEditorProps } from '../../shared/image-editor-protocol';
import { getImageDraftScope } from '../../shared/image-editor-drafts';
import { mockApi, session } from '../test-fixtures';
import { FileSidebar } from './FileSidebar';

const { discard, receivedScope } = vi.hoisted(() => ({ discard: vi.fn(), receivedScope: vi.fn() }));
vi.mock('./FileImageEditor', async () => ({
  ...(await import('../../shared/image-editor-drafts')),
  discardImageEditorDraft: discard,
  FileImageEditor: (props: FileImageEditorProps) => { receivedScope(props.draftScope); return <div aria-label="Mock image editor">
    <button onClick={() => props.onDirtyChange?.(true)}>Modify image</button>
    <button onClick={() => { void props.onSave('data:image/png;base64,YQ==').then(result => {
      if (result.ok) { props.onSaved?.(result.value, 'data:image/png;base64,YQ=='); props.onDirtyChange?.(false); }
    }); }}>Save image</button>
  </div>; },
}));
afterEach(cleanup);
function setup(mimeType: 'image/png' | 'image/gif' = 'image/png') {
  const item = session();
  const image: Extract<FileRead, { kind: 'image' }> = { kind: 'image', path: '/work/image.png', mimeType, dataUrl: `data:${mimeType};base64,Yg==`, revision: 'a'.repeat(64), size: 1 };
  const files = {
    list: vi.fn<FileApi['list']>(async () => success({ path: '/work', parentPath: '/', entries: [], truncated: false })),
    read: vi.fn<FileApi['read']>(async () => success(image)),
    write: vi.fn<FileApi['write']>(async input => success({ path: input.path, size: 1, revision: 'b'.repeat(64) })),
    writeImage: vi.fn<FileApi['writeImage']>(async input => success({ path: input.path, size: 2, revision: 'b'.repeat(64) })),
    readAsset: vi.fn<FileApi['readAsset']>(async () => success({ mimeType: 'image/png', dataUrl: 'data:image/png;base64,YQ==', size: 1 })),
  } satisfies FileApi;
  const api: ManagerApi = { ...mockApi().api, files };
  const onClose = vi.fn();
  render(<FileSidebar api={api} sessionId={item.id} tabId={item.tabs[0].id} onClose={onClose} openRequest={{ path: image.path, id: 1 }} />);
  return { item, image, files, onClose, user: userEvent.setup() };
}

describe('sidebar image editing integration', () => {
  it('passes original revision on save, then the returned revision on later saves', async () => {
    const f = setup();
    await f.user.click(await screen.findByRole('button', { name: 'Edit image' }));
    expect(receivedScope).toHaveBeenLastCalledWith(getImageDraftScope(f.files, f.item.id, f.item.tabs[0].id));
    await f.user.click(screen.getByRole('button', { name: 'Modify image' }));
    await f.user.click(screen.getByRole('button', { name: 'Save image' }));
    expect(f.files.writeImage).toHaveBeenCalledWith({ sessionId: f.item.id, tabId: f.item.tabs[0].id, path: f.image.path, revision: 'a'.repeat(64), dataUrl: 'data:image/png;base64,YQ==' });
    await f.user.click(screen.getByRole('button', { name: 'Modify image' }));
    await f.user.click(screen.getByRole('button', { name: 'Save image' }));
    expect(f.files.writeImage).toHaveBeenLastCalledWith(expect.objectContaining({ revision: 'b'.repeat(64) }));
    await f.user.click(screen.getByRole('button', { name: 'Preview image' }));
    expect(screen.getByRole('img', { name: 'image.png' })).toHaveAttribute('src', 'data:image/png;base64,YQ==');
  });
  it('guards dirty image close and discards the operation cache only with explicit confirmation', async () => {
    const f = setup();
    await f.user.click(await screen.findByRole('button', { name: 'Edit image' }));
    await f.user.click(screen.getByRole('button', { name: 'Modify image' }));
    await f.user.click(screen.getByRole('button', { name: 'Close file sidebar' }));
    expect(screen.getByRole('alertdialog')).toHaveTextContent('Save the image using its controls');
    expect(screen.queryByRole('button', { name: 'Save and continue' })).not.toBeInTheDocument();
    await f.user.click(screen.getByRole('button', { name: 'Cancel' }));
    expect(f.onClose).not.toHaveBeenCalled();
    await f.user.click(screen.getByRole('button', { name: 'Close file sidebar' }));
    await f.user.click(screen.getByRole('button', { name: 'Discard changes' }));
    expect(discard).toHaveBeenCalledWith(f.image, getImageDraftScope(f.files, f.item.id, f.item.tabs[0].id));
    expect(f.onClose).toHaveBeenCalledOnce();
  });
  it('keeps formats without an encoder preview-only', async () => {
    setup('image/gif');
    expect(await screen.findByRole('img', { name: 'image.png' })).toBeVisible();
    expect(screen.queryByRole('button', { name: 'Edit image' })).not.toBeInTheDocument();
  });
});
