// @vitest-environment jsdom
import { act, cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { failure, success, type ManagerApi } from '../../shared/contracts';
import { FILE_CHUNK_BYTES } from '../../shared/file-transfer-schemas';
import { mockApi } from '../test-fixtures';
import { Notifications } from '../components';
import { FileTransfers } from './FileTransfers';
import { localId, transferFixture } from './file-transfer-test-fixtures';

afterEach(cleanup);
function fixture() {
  const f = transferFixture();
  const api: ManagerApi = { ...mockApi().api, desktopFiles: f.local, files: {
    ...f.remote, list: vi.fn(async () => success({ path: '/folder', parentPath: '/', entries: [], truncated: false })),
    read: vi.fn(async () => failure('UNSUPPORTED', 'Preview is too large.')),
    write: vi.fn(async () => failure('UNSUPPORTED', 'Not used')),
    writeImage: vi.fn(async () => failure('UNSUPPORTED', 'Not used')),
    readAsset: vi.fn(async () => failure('UNSUPPORTED', 'Not used')),
  } };
  return { ...f, api };
}
const location = { path: '/folder' };
describe('desktop transfer toolbar', () => {
  it('uses icon-only labelled controls and requires a selected file for download', () => {
    const f = fixture(), view = render(<FileTransfers api={f.api} location={location} />);
    expect(screen.getByRole('button', { name: 'Upload file' }).textContent).toBe('');
    expect(screen.getByRole('button', { name: 'Download selected file' })).toHaveProperty('disabled', true);
    view.rerender(<FileTransfers api={f.api} location={location} selectedPath="/folder/selected.bin" />);
    expect(screen.getByRole('button', { name: 'Download selected file' })).toHaveProperty('disabled', false);
  });
  it('keeps inline folder controls upload-only and reports outcomes without adding a toolbar row', async () => {
    const f = fixture();
    const view = render(<Notifications><FileTransfers inline uploadOnly api={f.api} location={location} /></Notifications>);
    expect(screen.queryByRole('button', { name: 'Download selected file' })).toBeNull();
    fireEvent.click(screen.getByRole('button', { name: 'Upload file' }));
    await screen.findByText('Uploaded upload.bin');
    expect(view.container.querySelector('.file-transfers-inline > span')).toBeNull();
    f.remote.beginWrite.mockResolvedValueOnce(failure('FILE_CONFLICT', 'Already exists'));
    fireEvent.click(screen.getByRole('button', { name: 'Upload file' }));
    expect((await screen.findByRole('alert')).textContent).toContain('Already exists');
  });
  it('does not depend on a successful or supported file preview to download', async () => {
    const f = fixture();
    render(<FileTransfers api={f.api} location={location} selectedPath="/folder/unsupported.bin" />);
    fireEvent.click(screen.getByRole('button', { name: 'Download selected file' }));
    await screen.findByText('Downloaded unsupported.bin');
    expect(f.api.files?.read).not.toHaveBeenCalled(); expect(f.remote.beginRead).toHaveBeenCalledWith({ path: '/folder/unsupported.bin' });
    expect(f.local.finishDownload).toHaveBeenCalledOnce();
  });
  it('permits selected files over the preview cap to reach the native save picker', async () => {
    const f = fixture(), size = 70 * 1024 * 1024;
    f.remote.beginRead.mockImplementation(async input => success({ transferId: crypto.randomUUID(), path: input.path, size, revision: 'a'.repeat(64), kind: 'binary', mimeType: 'application/octet-stream', chunkBytes: FILE_CHUNK_BYTES }));
    f.remote.readChunk.mockResolvedValue(failure('ACCESS_DENIED', 'Stop after metadata for this test.'));
    render(<FileTransfers api={f.api} location={location} selectedPath="/folder/large.bin" />);
    fireEvent.click(screen.getByRole('button', { name: 'Download selected file' }));
    await screen.findByRole('alert'); expect(f.local.beginDownload).toHaveBeenCalledWith({ name: 'large.bin', size });
  });
  it('snapshots the host and destination folder while the native upload picker is open', async () => {
    const first = fixture(), second = fixture(), refresh = vi.fn();
    let choose = () => {};
    first.local.pickUpload.mockImplementationOnce(() => new Promise(resolve => { choose = () => resolve(success({ handle: localId, name: 'upload.bin', size: first.bytes.length, chunkBytes: FILE_CHUNK_BYTES })); }));
    const view = render(<FileTransfers api={first.api} location={{ path: '/first' }} onUploaded={refresh} />);
    fireEvent.click(screen.getByRole('button', { name: 'Upload file' }));
    view.rerender(<FileTransfers api={second.api} location={{ path: '/second' }} onUploaded={refresh} />);
    await act(async () => { choose(); }); await screen.findByText('Uploaded upload.bin');
    expect(first.remote.beginWrite).toHaveBeenCalledWith(expect.objectContaining({ path: '/first/upload.bin', mode: 'create' }));
    expect(second.remote.beginWrite).not.toHaveBeenCalled(); expect(second.local.pickUpload).not.toHaveBeenCalled(); expect(refresh).not.toHaveBeenCalled();
  });
  it('shows no-clobber errors and leaves existing remote files alone', async () => {
    const f = fixture(); f.remote.beginWrite.mockResolvedValueOnce(failure('FILE_CONFLICT', 'A file already exists at this destination.'));
    render(<FileTransfers api={f.api} location={location} />); fireEvent.click(screen.getByRole('button', { name: 'Upload file' }));
    expect((await screen.findByRole('alert')).textContent).toContain('already exists');
    expect(f.remote.finishWrite).not.toHaveBeenCalled(); expect(f.remote.beginWrite).toHaveBeenCalledOnce();
  });
  it('cancels pending transfers on explicit cancel', async () => {
    const f = fixture(); let finish = () => {};
    f.remote.writeChunk.mockImplementationOnce(() => new Promise(resolve => { finish = () => resolve(failure('ACCESS_DENIED', 'Transfer cancelled.')); }));
    const view = render(<FileTransfers api={f.api} location={location} />);
    fireEvent.click(screen.getByRole('button', { name: 'Upload file' }));
    await waitFor(() => expect(f.remote.writeChunk).toHaveBeenCalledOnce());
    fireEvent.click(screen.getByRole('button', { name: 'Cancel file transfer' }));
    expect(f.remote.cancelTransfer).toHaveBeenCalled(); expect(f.local.releaseUpload).toHaveBeenCalled();
    await act(async () => { finish(); }); await screen.findByText('Transfer cancelled');
    view.unmount(); expect(f.remote.finishWrite).not.toHaveBeenCalled();
  });
  it('reports an acknowledged commit rather than claiming cancellation won a finalization race', async () => {
    const f = fixture(); let finish = () => {};
    f.remote.finishWrite.mockImplementationOnce(() => new Promise(resolve => { finish = () => resolve(success({ path: '/folder/upload.bin', size: f.bytes.length, revision: 'a'.repeat(64) })); }));
    render(<FileTransfers api={f.api} location={location} />);
    fireEvent.click(screen.getByRole('button', { name: 'Upload file' }));
    await waitFor(() => expect(f.remote.finishWrite).toHaveBeenCalledOnce());
    fireEvent.click(screen.getByRole('button', { name: 'Cancel file transfer' }));
    await act(async () => { finish(); }); await screen.findByText('Uploaded upload.bin');
    expect(screen.queryByText('Transfer cancelled')).toBeNull();
  });
  it('releases host and native sink handles when an active download is unmounted', async () => {
    const f = fixture(); let finish = () => {};
    f.remote.readChunk.mockImplementationOnce(() => new Promise(resolve => { finish = () => resolve(failure('ACCESS_DENIED', 'Transfer cancelled.')); }));
    const view = render(<FileTransfers api={f.api} location={location} selectedPath="/folder/source.bin" />);
    fireEvent.click(screen.getByRole('button', { name: 'Download selected file' }));
    await waitFor(() => expect(f.remote.readChunk).toHaveBeenCalledOnce());
    const previous = f.remote.cancelTransfer.mock.calls.length;
    view.unmount(); expect(f.remote.cancelTransfer.mock.calls.length).toBeGreaterThan(previous);
    expect(f.local.cancelDownload).toHaveBeenCalled();
    await act(async () => { finish(); }); expect(f.local.finishDownload).not.toHaveBeenCalled();
  });
});
