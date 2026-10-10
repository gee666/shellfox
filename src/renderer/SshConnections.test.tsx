// @vitest-environment jsdom
import '@testing-library/jest-dom/vitest';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { act, cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import type { SshProfileDto } from '../shared/contracts';
import { failure, success } from '../shared/contracts';
import { SshConnections } from './SshConnections';
import { deferred, mockApi } from './test-fixtures';

const profile: SshProfileDto = { id: '00000000-0000-4000-8000-000000000201', name: 'prod-web', host: '10.0.0.5', port: 22, user: 'deploy', hasPassword: true, keyFile: '/keys/prod', remoteCwd: '/var/www', source: 'manual' };
afterEach(() => { cleanup(); vi.useRealTimers(); });
async function setup(profiles: SshProfileDto[] = []) {
  const fixture = mockApi();
  for (const item of profiles) await fixture.api.saveSshProfile({ ...item, password: item.hasPassword ? 'saved secret' : null });
  fixture.api.saveSshProfile.mockClear();
  const rendered = render(<SshConnections api={fixture.api} />);
  await waitFor(() => expect(screen.getByRole('button', { name: 'Add' })).toBeEnabled());
  return { ...fixture, ...rendered, user: userEvent.setup() };
}
async function add(user: ReturnType<typeof userEvent.setup>) {
  await user.click(screen.getByRole('button', { name: 'Add' }));
  await user.type(screen.getByLabelText('Name'), 'nas');
  await user.type(screen.getByLabelText('Host'), 'nas.local');
}

describe('SSH connections', () => {
  it('shows the empty state and CLI hint, adds immediately with defaults, and keeps passwords out of the list', async () => {
    const { api, user } = await setup();
    expect(screen.getByText('No connections yet.')).toBeVisible();
    expect(screen.getByText('shellfox ssh')).toBeVisible();
    await add(user);
    expect(screen.getByRole('form', { name: 'Add SSH connection' })).toBeVisible();
    await user.type(screen.getByLabelText('Password'), 'private secret');
    expect(screen.getByLabelText('Password')).toHaveAttribute('type', 'password');
    await user.type(screen.getByLabelText('Remote folder'), '/share{Enter}');
    await screen.findByRole('button', { name: 'Edit SSH connection nas' });
    expect(api.saveSshProfile).toHaveBeenCalledWith({ id: expect.any(String), name: 'nas', host: 'nas.local', port: 22, user: '', password: 'private secret', keyFile: null, remoteCwd: '/share' });
    expect(api.saveSettings).not.toHaveBeenCalled();
    expect(screen.queryByText('private secret')).not.toBeInTheDocument();
    expect(screen.queryByText('No connections yet.')).not.toBeInTheDocument();
  });
  it('edits in place, keeps an empty saved password as undefined, and replaces it when entered', async () => {
    const { api, user } = await setup([profile]);
    expect(screen.getByText('deploy@10.0.0.5')).toBeVisible();
    expect(screen.queryByText('deploy@10.0.0.5:22')).not.toBeInTheDocument();
    await user.click(screen.getByRole('button', { name: 'Edit SSH connection prod-web' }));
    expect(screen.getByLabelText('Name')).toHaveValue('prod-web');
    expect(screen.getByLabelText('Password')).toHaveValue('');
    expect(screen.getByLabelText('Password')).toHaveAttribute('placeholder', '•••••• saved');
    await user.clear(screen.getByLabelText('Port')); await user.type(screen.getByLabelText('Port'), '2222');
    await user.click(screen.getByRole('button', { name: 'Save' }));
    await screen.findByText('deploy@10.0.0.5:2222');
    expect(api.saveSshProfile.mock.calls[0]![0]).toMatchObject({ id: profile.id, password: undefined, port: 2222 });
    await user.click(screen.getByRole('button', { name: 'Edit prod-web' }));
    await user.type(screen.getByLabelText('Password'), 'replacement');
    await user.click(screen.getByRole('button', { name: 'Save' }));
    await waitFor(() => expect(api.saveSshProfile.mock.calls[1]![0].password).toBe('replacement'));
  });
  it('clears saved passwords explicitly with null and clears key files', async () => {
    const { api, user } = await setup([profile]);
    await user.click(screen.getByRole('button', { name: 'Edit prod-web' }));
    await user.click(screen.getByRole('button', { name: 'clear' }));
    expect(screen.getByLabelText('Password')).toHaveAttribute('placeholder', 'Password');
    await user.click(screen.getByRole('button', { name: 'Clear key file' }));
    await user.click(screen.getByRole('button', { name: 'Save' }));
    await waitFor(() => expect(api.saveSshProfile).toHaveBeenCalledWith(expect.objectContaining({ password: null, keyFile: null })));
  });
  it('cancels clear without changing the saved password, and allows only one editor', async () => {
    const { api, user } = await setup([profile]);
    await user.click(screen.getByRole('button', { name: 'Edit prod-web' }));
    await user.click(screen.getByRole('button', { name: 'clear' }));
    await user.keyboard('{Escape}');
    expect(api.saveSshProfile).not.toHaveBeenCalled();
    await user.click(screen.getByRole('button', { name: 'Edit prod-web' }));
    expect(screen.getByLabelText('Password')).toHaveAttribute('placeholder', '•••••• saved');
    await user.click(screen.getByRole('button', { name: 'Add' }));
    expect(screen.getAllByRole('form')).toHaveLength(1);
    expect(screen.getByLabelText('Name')).toHaveValue('');
    await user.click(screen.getByRole('button', { name: 'Cancel' }));
    expect(screen.queryByRole('form')).not.toBeInTheDocument();
  });
  it('uses the native key picker, handles cancellation and shows picker errors inline', async () => {
    const { api, user } = await setup();
    await add(user);
    api.chooseSshKeyFile.mockResolvedValueOnce(success({ path: '/keys/private.ppk' }));
    await user.click(screen.getByRole('button', { name: 'Choose key file' }));
    await waitFor(() => expect(screen.getByLabelText('Key file')).toHaveValue('/keys/private.ppk'));
    await user.click(screen.getByRole('button', { name: 'Choose key file' }));
    expect(screen.getByLabelText('Key file')).toHaveValue('/keys/private.ppk');
    api.chooseSshKeyFile.mockResolvedValueOnce(failure('INTERNAL', 'Key picker unavailable.'));
    await user.click(screen.getByRole('button', { name: 'Choose key file' }));
    expect(await screen.findByRole('alert')).toHaveTextContent('Key picker unavailable.');
  });
  it.each(['0', '65536', '1.5', 'abc'])('validates the port %s and required fields before saving', async port => {
    const { api, user } = await setup();
    await user.click(screen.getByRole('button', { name: 'Add' }));
    await user.type(screen.getByLabelText('Port'), port);
    await user.click(screen.getByRole('button', { name: 'Save' }));
    expect(screen.getByRole('alert')).toHaveTextContent('Name is required. Host is required. Port must be between 1 and 65535.');
    expect(api.saveSshProfile).not.toHaveBeenCalled();
  });
  it('disables Save while saving and shows backend name clashes inline without losing the draft', async () => {
    const { api, user } = await setup();
    const pending = deferred<Awaited<ReturnType<typeof api.saveSshProfile>>>();
    api.saveSshProfile.mockReturnValueOnce(pending.promise);
    await add(user); await user.click(screen.getByRole('button', { name: 'Save' }));
    expect(screen.getByRole('button', { name: 'Save' })).toBeDisabled();
    fireEvent.submit(screen.getByRole('form'));
    expect(api.saveSshProfile).toHaveBeenCalledTimes(1);
    await act(async () => pending.resolve(failure('VALIDATION', 'A connection with this name already exists.')));
    expect(screen.getByRole('alert')).toHaveTextContent('A connection with this name already exists.');
    expect(screen.getByLabelText('Name')).toHaveValue('nas');
    expect(screen.getByRole('button', { name: 'Save' })).toBeEnabled();
  });
  it('confirms removal inline, supports cancellation and only deletes after confirmation', async () => {
    const { api, user } = await setup([profile]);
    await user.click(screen.getByRole('button', { name: 'Remove prod-web' }));
    expect(screen.getByText('Remove prod-web?')).toBeVisible();
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
    expect(api.deleteSshProfile).not.toHaveBeenCalled();
    await user.click(screen.getByRole('button', { name: 'Cancel' }));
    await user.click(screen.getByRole('button', { name: 'Remove prod-web' }));
    api.deleteSshProfile.mockResolvedValueOnce(failure('STORAGE_FAILED', 'Cannot remove connection.'));
    await user.click(screen.getByRole('button', { name: 'Remove' }));
    expect(await screen.findByRole('alert')).toHaveTextContent('Cannot remove connection.');
    await user.click(screen.getByRole('button', { name: 'Remove' }));
    expect(await screen.findByText('No connections yet.')).toBeVisible();
    expect(api.deleteSshProfile).toHaveBeenCalledWith({ id: profile.id });
  });
  it.each([
    [{ added: 3, updated: 2, found: 5 }, '3 added, 2 updated'],
    [{ added: 0, updated: 0, found: 2 }, 'Already up to date'],
    [{ added: 0, updated: 0, found: 0 }, 'No PuTTY sessions found'],
  ])('shows import status %s transiently and updates the list', async (counts, message) => {
    const { api } = await setup();
    api.importPuttySessions.mockResolvedValueOnce(success({ profiles: [profile], ...counts }));
    vi.useFakeTimers();
    await act(async () => { fireEvent.click(screen.getByRole('button', { name: 'Import from PuTTY' })); });
    expect(screen.getByRole('status')).toHaveTextContent(message);
    expect(screen.getByRole('button', { name: 'Edit SSH connection prod-web' })).toBeVisible();
    await act(async () => vi.advanceTimersByTime(3100));
    expect(screen.queryByRole('status')).not.toBeInTheDocument();
    api.importPuttySessions.mockResolvedValueOnce(success({ profiles: [profile], ...counts }));
    await act(async () => { fireEvent.click(screen.getByRole('button', { name: 'Import from PuTTY' })); });
    expect(screen.getByRole('status')).toHaveTextContent(message);
  });
  it('shows load and import errors inline', async () => {
    const fixture = mockApi();
    fixture.api.listSshProfiles.mockResolvedValueOnce(failure('STORAGE_FAILED', 'Cannot read connections.'));
    render(<SshConnections api={fixture.api} />);
    expect(await screen.findByRole('alert')).toHaveTextContent('Cannot read connections.');
    fixture.api.importPuttySessions.mockResolvedValueOnce(failure('INTERNAL', 'Could not import PuTTY sessions.'));
    fireEvent.click(screen.getByRole('button', { name: 'Import from PuTTY' }));
    await waitFor(() => expect(screen.getByRole('alert')).toHaveTextContent('Could not import PuTTY sessions.'));
  });
});
