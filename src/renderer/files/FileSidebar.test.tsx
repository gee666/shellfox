// @vitest-environment jsdom
import '@testing-library/jest-dom/vitest';
import { act, cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, describe, expect, it, vi } from 'vitest';
import type { ManagerApi, Result } from '../../shared/contracts';
import { failure, success } from '../../shared/contracts';
import type { FileApi, FileRead, FileWriteResult } from '../../shared/file-contracts';
import { deferred, mockApi, session } from '../test-fixtures';
import { FileSidebar } from './FileSidebar';

afterEach(cleanup);
const item = session();
const revision = 'a'.repeat(64);
function textFile(path = '/work/main.ts', text = 'const answer = 42;'): FileRead {
  return { kind: 'text', path, text, revision, mimeType: 'text/plain', size: text.length };
}
function fixture(initial: FileRead = textFile()) {
  const files = {
    list: vi.fn<FileApi['list']>(async () => success({ path: '/work', parentPath: '/', truncated: false, entries: [
      { name: 'main.ts', path: '/work/main.ts', kind: 'file' },
      { name: 'next.ts', path: '/work/next.ts', kind: 'file' },
      { name: 'src', path: '/work/src', kind: 'directory' },
    ] })),
    read: vi.fn<FileApi['read']>(async () => success(initial)),
    write: vi.fn<FileApi['write']>(async input => success({ path: input.path, revision: 'b'.repeat(64), size: input.text.length })),
    writeImage: vi.fn<FileApi['writeImage']>(async input => success({ path: input.path, revision: 'b'.repeat(64), size: 12 })),
    readAsset: vi.fn<FileApi['readAsset']>(async () => success({ mimeType: 'text/javascript', dataUrl: 'data:text/javascript;base64,YWxlcnQoMSk=', size: 8 })),
  } satisfies FileApi;
  const api: ManagerApi = { ...mockApi().api, files };
  const onClose = vi.fn();
  const props = { api, sessionId: item.id, tabId: item.tabs[0].id, onClose, openRequest: { path: initial.path, id: 1 } };
  return { api, files, onClose, props, user: userEvent.setup() };
}

async function edit(text = 'changed') {
  const input = await screen.findByRole('textbox', { name: 'File contents' });
  fireEvent.change(input, { target: { value: text } });
  return input;
}

describe('file sidebar', () => {
  it('loads a scoped directory and file, highlights and saves text with its revision', async () => {
    const f = fixture(); render(<FileSidebar {...f.props} />);
    const input = await screen.findByRole('textbox', { name: 'File contents' });
    expect(f.files.read).toHaveBeenCalledWith({ sessionId: item.id, tabId: item.tabs[0].id, path: '/work/main.ts' });
    expect(f.files.list).toHaveBeenCalledWith({ sessionId: item.id, tabId: item.tabs[0].id, path: '.' });
    expect(input.previousElementSibling?.querySelector('.token.keyword')).toHaveTextContent('const');
    fireEvent.change(input, { target: { value: 'const answer = 43;' } });
    fireEvent.keyDown(input, { key: 's', ctrlKey: true });
    await waitFor(() => expect(f.files.write).toHaveBeenCalledWith({ sessionId: item.id, tabId: item.tabs[0].id, path: '/work/main.ts', text: 'const answer = 43;', revision }));
    await waitFor(() => expect(screen.getByRole('button', { name: 'Save file' })).toBeDisabled());
  });

  it('protects navigation and close, supports cancel and explicit discard', async () => {
    const f = fixture(); render(<FileSidebar {...f.props} />);
    await edit();
    await f.user.click(screen.getByRole('button', { name: 'Close file sidebar' }));
    const prompt = screen.getByRole('alertdialog', { name: 'Unsaved changes' });
    expect(within(prompt).getByRole('button', { name: 'Cancel' })).toHaveFocus();
    await f.user.keyboard('{Escape}');
    expect(screen.queryByRole('alertdialog')).not.toBeInTheDocument();
    expect(f.onClose).not.toHaveBeenCalled();
    f.files.read.mockResolvedValueOnce(success(textFile('/work/next.ts', 'next')));
    await f.user.click(screen.getByRole('button', { name: 'next.ts' }));
    expect(f.files.read).toHaveBeenCalledTimes(1);
    await f.user.click(screen.getByRole('button', { name: 'Discard changes' }));
    expect(await screen.findByDisplayValue('next')).toBeVisible();
  });

  it('does not navigate after a failed save and keeps the draft', async () => {
    const f = fixture(); f.files.write.mockResolvedValue(failure('STORAGE_FAILED', 'Disk is full'));
    render(<FileSidebar {...f.props} />); await edit();
    await f.user.click(screen.getByRole('button', { name: 'Close file sidebar' }));
    await f.user.click(screen.getByRole('button', { name: 'Save and continue' }));
    expect(await screen.findByRole('alert')).toHaveTextContent('Disk is full');
    expect(f.onClose).not.toHaveBeenCalled();
    expect(screen.getByRole('textbox', { name: 'File contents' })).toHaveValue('changed');
    await f.user.click(screen.getByRole('button', { name: 'Cancel' }));
  });

  it('reloads an explicitly selected clean file after conflict, discard, close and reopen', async () => {
    const f = fixture(); f.files.write.mockResolvedValueOnce(failure('FILE_CONFLICT', 'File changed on disk'));
    const view = render(<FileSidebar {...f.props} />); await edit('unsaved draft');
    await f.user.click(screen.getByRole('button', { name: 'Save file' }));
    expect(await screen.findByRole('alert')).toHaveTextContent('File changed on disk');
    await f.user.click(screen.getByRole('button', { name: 'Close file sidebar' }));
    await f.user.click(screen.getByRole('button', { name: 'Discard changes' }));
    expect(f.onClose).toHaveBeenCalledOnce();
    view.rerender(<FileSidebar {...f.props} hidden />);
    f.files.read.mockResolvedValueOnce(success({ ...textFile('/work/main.ts', 'external edit'), revision: 'c'.repeat(64) }));
    view.rerender(<FileSidebar {...f.props} />);
    await f.user.click(screen.getByRole('button', { name: 'main.ts' }));
    expect(await screen.findByDisplayValue('external edit')).toBeVisible();
    expect(f.files.read).toHaveBeenCalledTimes(2);
  });

  it('retains drafts and original revision across unmounts, detects external changes', async () => {
    const f = fixture(); const first = render(<FileSidebar {...f.props} />); await edit('retained');
    first.unmount();
    f.files.read.mockResolvedValue(success({ ...textFile(), revision: 'c'.repeat(64) }));
    render(<FileSidebar {...f.props} openRequest={undefined} />);
    expect(await screen.findByDisplayValue('retained')).toBeVisible();
    expect(screen.getByRole('alert')).toHaveTextContent('changed on disk');
    await f.user.click(screen.getByRole('button', { name: 'Save file' }));
    await waitFor(() => expect(f.files.write).toHaveBeenCalledWith(expect.objectContaining({ text: 'retained', revision })));
  });

  it('preserves edits and undo-to-original made while a save is in flight', async () => {
    const f = fixture(); const pending = deferred<Result<FileWriteResult>>();
    f.files.write.mockReturnValueOnce(pending.promise);
    render(<FileSidebar {...f.props} />); const input = await edit('saving');
    await f.user.click(screen.getByRole('button', { name: 'Save file' }));
    fireEvent.change(input, { target: { value: 'const answer = 42;' } });
    await act(async () => pending.resolve(success({ path: '/work/main.ts', revision: 'd'.repeat(64), size: 6 })));
    expect(input).toHaveValue('const answer = 42;');
    expect(screen.getByRole('button', { name: 'Save file' })).toBeEnabled();
    await f.user.click(screen.getByRole('button', { name: 'Save file' }));
    expect(f.files.write).toHaveBeenLastCalledWith(expect.objectContaining({ text: 'const answer = 42;', revision: 'd'.repeat(64) }));
  });

  it('ignores a stale read when another request is opened', async () => {
    const f = fixture(); const pending = deferred<Result<FileRead>>();
    f.files.read.mockReturnValueOnce(pending.promise).mockResolvedValueOnce(success(textFile('/work/next.ts', 'newer')));
    const view = render(<FileSidebar {...f.props} />);
    expect(screen.getByText('Loading file…')).toBeVisible();
    view.rerender(<FileSidebar {...f.props} openRequest={{ path: '/work/next.ts', id: 2 }} />);
    expect(await screen.findByDisplayValue('newer')).toBeVisible();
    await act(async () => pending.resolve(success(textFile('/work/main.ts', 'stale'))));
    expect(screen.queryByDisplayValue('stale')).not.toBeInTheDocument();
  });

  it('restores a retained draft even when the file has been deleted', async () => {
    const f = fixture(); const view = render(<FileSidebar {...f.props} />); await edit('keep me');
    view.unmount(); f.files.read.mockResolvedValue(failure('NOT_FOUND', 'File no longer exists'));
    render(<FileSidebar {...f.props} />);
    expect(await screen.findByDisplayValue('keep me')).toBeVisible();
    expect(screen.getByText('File no longer exists')).toBeVisible();
  });

  it('retains newer edits across an unmount while an older save finishes', async () => {
    const f = fixture(); const pending = deferred<Result<FileWriteResult>>();
    f.files.write.mockReturnValueOnce(pending.promise);
    const first = render(<FileSidebar {...f.props} />); await edit('saving');
    await f.user.click(screen.getByRole('button', { name: 'Save file' }));
    first.unmount(); render(<FileSidebar {...f.props} />); await edit('new draft');
    expect(screen.getByRole('button', { name: 'Save file' })).toBeDisabled();
    await act(async () => pending.resolve(success({ path: '/work/main.ts', revision: 'e'.repeat(64), size: 6 })));
    expect(screen.getByRole('textbox', { name: 'File contents' })).toHaveValue('new draft');
    await f.user.click(screen.getByRole('button', { name: 'Save file' }));
    expect(f.files.write).toHaveBeenLastCalledWith(expect.objectContaining({ text: 'new draft', revision: 'e'.repeat(64) }));
  });

  it('reveals another line in the same dirty file without losing edits or prompting', async () => {
    const f = fixture(); const view = render(<FileSidebar {...f.props} />); await edit('one\ntwo');
    view.rerender(<FileSidebar {...f.props} openRequest={{ path: '/work/main.ts', id: 2, line: 2 }} />);
    expect(screen.queryByRole('alertdialog')).not.toBeInTheDocument();
    expect(screen.getByRole('textbox', { name: 'File contents' })).toHaveValue('one\ntwo');
    expect(f.files.read).toHaveBeenCalledTimes(1);
  });

  it('previews SVG as a non-scriptable image and keeps its source editable', async () => {
    const f = fixture(textFile('/work/art.svg', '<svg xmlns="http://www.w3.org/2000/svg"><script>alert(1)</script></svg>'));
    render(<FileSidebar {...f.props} />);
    expect(await screen.findByRole('img', { name: 'art.svg' })).toHaveAttribute('src', expect.stringContaining('data:image/svg+xml'));
    expect(screen.queryByTitle('HTML preview')).not.toBeInTheDocument();
    await f.user.click(screen.getByRole('button', { name: 'Source' }));
    expect(screen.getByRole('textbox', { name: 'File contents' })).toHaveTextContent('<svg');
  });

  it('renders Markdown safely with GFM tables, strips metadata and can switch to source', async () => {
    const f = fixture(textFile('/work/README.md', '---\ntitle: hidden\n---\n# Visible\n\n|a|b|\n|-|-|\n|1|2|\n\n<script>alert(1)</script>'));
    render(<FileSidebar {...f.props} />);
    expect(await screen.findByRole('heading', { name: 'Visible' })).toBeVisible();
    expect(screen.getByRole('table')).toBeVisible();
    expect(screen.getByRole('article', { name: 'Markdown preview' }).querySelector('script')).toBeNull();
    expect(screen.queryByText('title: hidden')).not.toBeInTheDocument();
    await f.user.click(screen.getByRole('button', { name: 'Source' }));
    expect(screen.getByRole('textbox', { name: 'File contents' })).toHaveTextContent('title: hidden');
  });

  it('renders HTML with sibling scripts in an opaque sandbox and blocks app/network privileges', async () => {
    const f = fixture(textFile('/work/index.html', '<script src="app.js"></script><button>Run</button>'));
    render(<FileSidebar {...f.props} />);
    const frame = await screen.findByTitle('HTML preview');
    expect(frame).toHaveAttribute('sandbox', 'allow-scripts');
    const source = frame.getAttribute('src') ?? '';
    expect(source).toMatch(/^shellfox-preview:\/\/document\/\?html=/);
    expect(decodeURIComponent(source)).toContain('data:text/javascript;base64,YWxlcnQoMSk=');
    expect(decodeURIComponent(source)).toContain("connect-src 'none'");
    expect(f.files.readAsset).toHaveBeenCalledWith({ sessionId: item.id, tabId: item.tabs[0].id, path: '/work/index.html', assetPath: 'app.js' });
  });

  it('reports an oversized HTML preview URL without crashing or disabling Source and download controls', async () => {
    const f = fixture(textFile('/work/large.html', `<p>${'界'.repeat(1_500_000)}</p>`));
    render(<FileSidebar {...f.props} />);
    expect(await screen.findByRole('alert')).toHaveTextContent('Preview assets exceed the document size limit');
    expect(screen.queryByTitle('HTML preview')).not.toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Source' })).toBeEnabled();
    expect(screen.getByRole('button', { name: 'Download selected file' })).toBeInTheDocument();
    await f.user.click(screen.getByRole('button', { name: 'Source' }));
    expect(screen.getByRole('textbox', { name: 'File contents' })).toBeVisible();
  });

  it('shows failed assets without throwing away the HTML document', async () => {
    const f = fixture(textFile('/work/index.html', '<script src="missing.js"></script><p>Hello</p>'));
    f.files.readAsset.mockResolvedValue(failure('NOT_FOUND', 'Missing asset'));
    render(<FileSidebar {...f.props} />);
    expect(await screen.findByTitle('HTML preview')).toBeVisible();
    await f.user.click(screen.getByText('1 unavailable preview assets'));
    expect(screen.getByText('missing.js: Missing asset')).toBeVisible();
  });

  it('previews images and explains unsupported binary files', async () => {
    const image = fixture({ kind: 'image', path: '/work/picture.png', mimeType: 'image/png', size: 4, revision, dataUrl: 'data:image/png;base64,YQ==' });
    const view = render(<FileSidebar {...image.props} />);
    expect(await screen.findByRole('img', { name: 'picture.png' })).toHaveAttribute('src', 'data:image/png;base64,YQ==');
    view.unmount();
    const binary = fixture({ kind: 'binary', path: '/work/archive.zip', mimeType: 'application/octet-stream', size: 100, revision });
    render(<FileSidebar {...binary.props} />);
    expect(await screen.findByText(/Binary file/)).toHaveTextContent('100 bytes');
  });

  it('supports loading errors, file retry, folder retry and folder expansion', async () => {
    const f = fixture();
    f.files.list.mockResolvedValueOnce(failure('ACCESS_DENIED', 'Folder denied'));
    f.files.read.mockRejectedValueOnce(new Error('File denied'));
    render(<FileSidebar {...f.props} />);
    expect(await screen.findByText('File denied')).toBeVisible();
    await f.user.click(screen.getByRole('button', { name: 'Retry file' }));
    expect(await screen.findByRole('textbox', { name: 'File contents' })).toBeVisible();
    await f.user.click(screen.getByRole('button', { name: 'Retry folder' }));
    await f.user.click(await screen.findByRole('button', { name: 'src' }));
    await waitFor(() => expect(f.files.list).toHaveBeenCalledWith(expect.objectContaining({ path: '/work/src' })));
  });

  it('opens a requested source location and retains dirty text while hidden', async () => {
    const f = fixture(textFile('/work/main.ts', 'one\ntwo\nthree'));
    const view = render(<FileSidebar {...f.props} openRequest={{ path: '/work/main.ts', id: 1, line: 2, column: 2 }} />);
    const input = await screen.findByRole('textbox', { name: 'File contents' });
    expect(input).toHaveFocus(); expect(input).toHaveProperty('selectionStart', 5);
    fireEvent.change(input, { target: { value: 'still here' } });
    view.rerender(<FileSidebar {...f.props} hidden />);
    expect(screen.getByLabelText('File sidebar')).not.toBeVisible();
    view.rerender(<FileSidebar {...f.props} />);
    expect(input).toHaveValue('still here');
  });
});
