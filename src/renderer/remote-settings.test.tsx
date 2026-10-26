// @vitest-environment jsdom
import '@testing-library/jest-dom/vitest';
import './terminal-test-mocks';
import { afterEach, expect, it, vi } from 'vitest';
import { act, cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import type { RemoteState } from '../shared/remote-contracts';
import { failure, success } from '../shared/contracts';
import { RemoteSettings } from './RemoteSettings';
import { createRemoteController, type RemoteController } from './remote-controller';
import { deferred, mockApi } from './test-fixtures';
import { remoteFixture } from './remote-test-fixtures';

const controllers: RemoteController[] = [];
afterEach(() => { cleanup(); controllers.splice(0).forEach(controller => controller.stop()); vi.useRealTimers(); });
async function setup(status: RemoteState['status'] = 'online') {
  const local = mockApi(), remote = remoteFixture();
  remote.state.status = status;
  remote.state.peers.push({ id: 'other', hostname: 'Other device', addresses: ['100.64.0.3'], online: true, connection: 'disconnected', approved: false, remembered: false });
  remote.state.pending.push({ id: 'pending', hostname: 'Pending laptop', addresses: ['100.64.0.4'], online: true });
  remote.state.approvals.push({ peerId: 'approved', hostname: 'Approved laptop', approvedAt: '2026-10-04T10:00:00.000Z' });
  const openExternal = vi.fn(async () => success({ opened: true as const }));
  const controller = createRemoteController(remote.api, local.api); controllers.push(controller); controller.start();
  const view = render(<RemoteSettings controller={controller} local={{ ...local.api, openExternal }} />);
  await screen.findByLabelText('Remote machine name');
  const user = userEvent.setup();
  const emit = async (patch: Partial<RemoteState>) => { await act(async () => { remote.emit({ type: 'state', state: { ...remote.state, ...patch } }); }); };
  return { ...view, local, remote, controller, user, openExternal, emit };
}

it('shows an already connected account without a pointless login action or browser launch', async () => {
  const f = await setup();
  expect(screen.getByRole('status')).toHaveTextContent('Connected');
  expect(screen.queryByRole('button', { name: 'Sign in to Tailscale' })).not.toBeInTheDocument();
  expect(screen.queryByText('Advanced sign-in')).not.toBeInTheDocument();
  expect(screen.queryByLabelText('Tailscale auth key')).not.toBeInTheDocument();
  expect(screen.queryByRole('alert')).not.toBeInTheDocument(); expect(f.openExternal).not.toHaveBeenCalled();
  await f.emit({ loginUrl: 'https://login.tailscale.com/stale' });
  expect(f.openExternal).not.toHaveBeenCalled();
});

it('connects discovery machines, forgets saved machines, and keeps failures inline', async () => {
  const { remote, user } = await setup();
  vi.mocked(remote.api.connect).mockResolvedValueOnce(failure('NATIVE_UNAVAILABLE', 'Shellfox is not running on this device.'));
  await user.click(screen.getByRole('button', { name: 'Connect Other device' }));
  expect(remote.api.connect).toHaveBeenCalledWith({ peerId: 'other' });
  expect(await screen.findByRole('alert')).toHaveTextContent('Shellfox is not running on this device.');
  await user.click(screen.getByRole('button', { name: 'Forget Linux server' }));
  expect(remote.api.forget).toHaveBeenCalledWith({ peerId: 'server' });
});

it('immediately shows sign-in progress and automatically opens a pushed login URL exactly once', async () => {
  const f = await setup('login-required');
  const pending = deferred<Awaited<ReturnType<typeof f.remote.api.login>>>();
  vi.mocked(f.remote.api.login).mockReturnValueOnce(pending.promise);
  await f.user.click(screen.getByRole('button', { name: 'Sign in to Tailscale' }));
  expect(screen.getByRole('status')).toHaveTextContent('Signing in');
  expect(screen.getByRole('button', { name: 'Cancel sign-in' })).toBeVisible();
  await f.emit({ loginUrl: 'https://login.tailscale.com/new' });
  expect(f.openExternal).toHaveBeenCalledExactlyOnceWith({ url: 'https://login.tailscale.com/new' });
  expect(screen.getByText(/Finish signing in in your browser/)).toBeVisible();
  await f.emit({ loginUrl: 'https://login.tailscale.com/new' });
  await act(async () => pending.resolve(success({ ...f.remote.state, loginUrl: 'https://login.tailscale.com/new' })));
  expect(f.openExternal).toHaveBeenCalledTimes(1);
  await f.emit({ status: 'online', loginUrl: null });
  expect(screen.getByRole('status')).toHaveTextContent('Connected');
  expect(screen.queryByRole('button', { name: 'Cancel sign-in' })).not.toBeInTheDocument();
  expect(screen.queryByRole('button', { name: 'Continue in browser' })).not.toBeInTheDocument();
});

it('updates Continue to a fresh pushed sign-in address without opening another browser window', async () => {
  const f = await setup('login-required');
  const pending = deferred<Awaited<ReturnType<typeof f.remote.api.login>>>();
  vi.mocked(f.remote.api.login).mockReturnValueOnce(pending.promise);
  await f.user.click(screen.getByRole('button', { name: 'Sign in to Tailscale' }));
  await f.emit({ loginUrl: 'https://login.tailscale.com/first' });
  expect(f.openExternal).toHaveBeenCalledExactlyOnceWith({ url: 'https://login.tailscale.com/first' });
  await f.emit({ loginUrl: 'https://login.tailscale.com/fresh' });
  expect(screen.getByText('Sign-in address updated. Continue in browser.')).toBeVisible();
  expect(f.openExternal).toHaveBeenCalledTimes(1);
  await act(async () => pending.resolve(success({ ...f.remote.state, loginUrl: 'https://login.tailscale.com/first' })));
  await f.user.click(screen.getByRole('button', { name: 'Continue in browser' }));
  expect(f.openExternal).toHaveBeenLastCalledWith({ url: 'https://login.tailscale.com/fresh' });
  expect(f.openExternal).toHaveBeenCalledTimes(2);
});

it('keeps a fresh URL when opening the old address fails asynchronously', async () => {
  const f = await setup('login-required');
  const browser = deferred<Awaited<ReturnType<typeof f.openExternal>>>();
  f.openExternal.mockReturnValueOnce(browser.promise);
  vi.mocked(f.remote.api.login).mockResolvedValueOnce(success({ ...f.remote.state, loginUrl: 'https://login.tailscale.com/first' }));
  await f.user.click(screen.getByRole('button', { name: 'Sign in to Tailscale' }));
  await f.emit({ loginUrl: 'https://login.tailscale.com/fresh' });
  await act(async () => browser.resolve(failure('INTERNAL', 'Browser blocked.')));
  expect(screen.getByLabelText('Tailscale login address')).toHaveValue('https://login.tailscale.com/fresh');
  await f.user.click(screen.getByRole('button', { name: 'Continue in browser' }));
  expect(f.openExternal).toHaveBeenLastCalledWith({ url: 'https://login.tailscale.com/fresh' });
});

it('never automatically opens a stale URL on initial render, retry setup, rerender or remount', async () => {
  const f = await setup('login-required');
  await f.emit({ loginUrl: 'https://login.tailscale.com/old' });
  expect(f.openExternal).not.toHaveBeenCalled();
  const pending = deferred<Awaited<ReturnType<typeof f.remote.api.login>>>();
  vi.mocked(f.remote.api.login).mockReturnValueOnce(pending.promise);
  await f.user.click(screen.getByRole('button', { name: 'Sign in to Tailscale' }));
  await f.emit({ loginUrl: 'https://login.tailscale.com/old' });
  expect(f.openExternal).not.toHaveBeenCalled();
  await f.user.click(screen.getByRole('button', { name: 'Cancel sign-in' }));
  await f.emit({ loginUrl: 'https://login.tailscale.com/later' });
  expect(f.openExternal).not.toHaveBeenCalled();
  f.unmount(); render(<RemoteSettings controller={f.controller} local={{ ...f.local.api, openExternal: f.openExternal }} />);
  expect(f.openExternal).not.toHaveBeenCalled();
  await act(async () => pending.resolve(success({ ...f.remote.state, loginUrl: 'https://login.tailscale.com/later' })));
  expect(f.openExternal).not.toHaveBeenCalled();
});

it('provides obvious Continue and Retry actions when opening the browser fails', async () => {
  const f = await setup('login-required');
  f.openExternal.mockResolvedValueOnce(failure('INTERNAL', 'Browser blocked.'));
  vi.mocked(f.remote.api.login).mockResolvedValueOnce(success({ ...f.remote.state, loginUrl: 'https://login.tailscale.com/new' }));
  await f.user.click(screen.getByRole('button', { name: 'Sign in to Tailscale' }));
  expect(await screen.findByRole('alert')).toHaveTextContent('Could not open your browser');
  expect(screen.getByRole('button', { name: 'Retry sign-in' })).toBeVisible();
  expect(screen.getByLabelText('Tailscale login address')).toHaveValue('https://login.tailscale.com/new');
  await f.user.click(screen.getByRole('button', { name: 'Continue in browser' }));
  expect(f.openExternal).toHaveBeenCalledTimes(2);
  expect(screen.queryByRole('alert')).not.toBeInTheDocument();
});

it('makes sign-in transport failure actionable without retaining a busy state', async () => {
  const f = await setup('login-required');
  vi.mocked(f.remote.api.login).mockRejectedValueOnce(new Error('disconnected'));
  await f.user.click(screen.getByRole('button', { name: 'Sign in to Tailscale' }));
  expect(await screen.findByRole('alert')).toHaveTextContent('Could not start sign-in');
  expect(screen.getByRole('button', { name: 'Retry sign-in' })).toBeEnabled();
  expect(screen.queryByRole('button', { name: 'Cancel sign-in' })).not.toBeInTheDocument();
});

it('times out an unfinished browser flow and lets the user retry', async () => {
  const f = await setup('login-required');
  vi.mocked(f.remote.api.login).mockResolvedValueOnce(success({ ...f.remote.state, loginUrl: 'https://login.tailscale.com/new' }));
  vi.useFakeTimers();
  await act(async () => { fireEvent.click(screen.getByRole('button', { name: 'Sign in to Tailscale' })); });
  await f.emit({ loginUrl: 'https://login.tailscale.com/fresh' });
  await act(async () => { await vi.advanceTimersByTimeAsync(120_000); });
  expect(screen.getByRole('alert')).toHaveTextContent('Sign-in timed out');
  expect(screen.getByRole('button', { name: 'Retry sign-in' })).toBeEnabled();
  expect(screen.getByLabelText('Tailscale login address')).toHaveValue('https://login.tailscale.com/fresh');
});

it('keeps the latest URL when a manually continued sign-in times out', async () => {
  const f = await setup('login-required');
  await f.emit({ loginUrl: 'https://login.tailscale.com/first' });
  vi.useFakeTimers();
  await act(async () => { fireEvent.click(screen.getByRole('button', { name: 'Continue in browser' })); });
  await f.emit({ loginUrl: 'https://login.tailscale.com/fresh' });
  await act(async () => { await vi.advanceTimersByTimeAsync(120_000); });
  expect(screen.getByLabelText('Tailscale login address')).toHaveValue('https://login.tailscale.com/fresh');
  await act(async () => { fireEvent.click(screen.getByRole('button', { name: 'Continue in browser' })); });
  expect(f.openExternal).toHaveBeenLastCalledWith({ url: 'https://login.tailscale.com/fresh' });
});

it('accepts a one-use auth key under Advanced without saving or retaining it', async () => {
  const f = await setup('login-required');
  await f.user.click(screen.getByText('Advanced sign-in'));
  await f.user.type(screen.getByLabelText('Tailscale auth key'), 'tskey-auth-secret');
  await f.user.click(screen.getByRole('button', { name: 'Use key' }));
  expect(f.remote.api.login).toHaveBeenLastCalledWith({ authKey: 'tskey-auth-secret' });
  expect(screen.getByLabelText('Tailscale auth key')).toHaveValue('');
  expect(f.local.api.saveSettings).not.toHaveBeenCalled();
});

it('never automatically opens browser URLs for an auth-key attempt while enrollment is pending', async () => {
  const f = await setup('login-required');
  await f.emit({ loginUrl: 'https://login.tailscale.com/cached' });
  vi.mocked(f.remote.api.login).mockResolvedValueOnce(success({ ...f.remote.state, loginUrl: 'https://login.tailscale.com/transient' }));
  await f.user.click(screen.getByText('Advanced sign-in'));
  await f.user.type(screen.getByLabelText('Tailscale auth key'), 'tskey-auth-secret');
  await f.user.click(screen.getByRole('button', { name: 'Use key' }));
  expect(f.remote.api.login).toHaveBeenLastCalledWith({ authKey: 'tskey-auth-secret' });
  expect(screen.getByRole('status')).toHaveTextContent('Signing in');
  expect(f.openExternal).not.toHaveBeenCalled();
  await f.emit({ loginUrl: 'https://login.tailscale.com/another-transient' });
  expect(f.openExternal).not.toHaveBeenCalled();
  expect(screen.getByRole('button', { name: 'Continue in browser' })).toBeVisible();
  await f.emit({ status: 'online', loginUrl: null });
  expect(screen.getByRole('status')).toHaveTextContent('Connected');
  expect(screen.queryByRole('button', { name: 'Cancel sign-in' })).not.toBeInTheDocument();
  expect(screen.queryByRole('button', { name: 'Continue in browser' })).not.toBeInTheDocument();
  expect(f.openExternal).not.toHaveBeenCalled();
});

it('configures the local remote service and supports approval, rejection and confirmed revocation', async () => {
  const f = await setup();
  await f.user.click(screen.getByRole('switch', { name: 'Allow remote access' }));
  expect(f.remote.api.configure).toHaveBeenCalledWith({ enabled: false, hostname: 'local' });
  await f.user.clear(screen.getByRole('textbox', { name: 'Remote machine name' }));
  await f.user.type(screen.getByRole('textbox', { name: 'Remote machine name' }), 'my-machine');
  await f.user.click(screen.getByRole('button', { name: 'Save' }));
  expect(f.remote.api.configure).toHaveBeenLastCalledWith({ enabled: true, hostname: 'my-machine' });
  await f.user.click(screen.getByRole('button', { name: 'Approve Pending laptop' }));
  expect(f.remote.api.approve).toHaveBeenCalledWith({ peerId: 'pending' });
  await f.user.click(screen.getByRole('button', { name: 'Reject Pending laptop' }));
  expect(f.remote.api.reject).toHaveBeenCalledWith({ peerId: 'pending' });
  await f.user.click(screen.getByRole('button', { name: 'Revoke Approved laptop' }));
  expect(f.remote.api.revoke).not.toHaveBeenCalled();
  await f.user.click(screen.getByRole('button', { name: 'Confirm revoke Approved laptop' }));
  expect(f.remote.api.revoke).toHaveBeenCalledWith({ peerId: 'approved' });
});

it('distinguishes same-name clients by full authenticated IDs for approval and revocation', async () => {
  const f = await setup();
  const pending = [1, 2].map(index => ({ id: `node-pending-${index}`, hostname: 'Shared machine', addresses: [], online: true }));
  const approvals = [1, 2].map(index => ({ peerId: `node-approved-${index}`, hostname: 'Shared machine', approvedAt: '2026-10-04T10:00:00.000Z' }));
  await f.emit({ pending, approvals });
  for (const id of ['node-pending-1', 'node-pending-2', 'node-approved-1', 'node-approved-2']) expect(screen.getByText(`Device ID: ${id}`)).toBeVisible();
  const approve = screen.getAllByRole('button', { name: 'Approve Shared machine' });
  expect(approve[0]).toHaveAccessibleDescription('Device ID: node-pending-1');
  expect(approve[1]).toHaveAccessibleDescription('Device ID: node-pending-2');
  const revoke = screen.getAllByRole('button', { name: 'Revoke Shared machine' });
  expect(revoke[0]).toHaveAccessibleDescription('Device ID: node-approved-1');
  expect(revoke[1]).toHaveAccessibleDescription('Device ID: node-approved-2');
  await f.user.click(approve[1]!); expect(f.remote.api.approve).toHaveBeenCalledWith({ peerId: 'node-pending-2' });
  await f.emit({ pending, approvals });
  await f.user.click(screen.getAllByRole('button', { name: 'Revoke Shared machine' })[1]!);
  const confirm = screen.getByRole('button', { name: 'Confirm revoke Shared machine' });
  expect(confirm).toHaveAccessibleDescription('Device ID: node-approved-2');
  await f.user.click(confirm); expect(f.remote.api.revoke).toHaveBeenCalledWith({ peerId: 'node-approved-2' });
});

it('shows requester approval and declined states with cancellation and a fresh access request', async () => {
  const f = await setup();
  await f.emit({ peers: f.remote.state.peers.map(peer => peer.id === 'other' ? { ...peer, connection: 'awaiting-approval' } : peer) });
  expect(screen.getByText('Waiting for approval', { selector: 'small' })).toBeVisible();
  await f.user.click(screen.getByRole('button', { name: 'Cancel request to Other device' }));
  expect(f.remote.api.disconnect).toHaveBeenCalledWith({ peerId: 'other' });
  await f.emit({ peers: f.remote.state.peers.map(peer => peer.id === 'other' ? { ...peer, connection: 'rejected' } : peer) });
  expect(screen.getByText('Request declined')).toBeVisible();
  await f.user.click(screen.getByRole('button', { name: 'Request access to Other device again' }));
  expect(f.remote.api.connect).toHaveBeenCalledWith({ peerId: 'other' });
});
