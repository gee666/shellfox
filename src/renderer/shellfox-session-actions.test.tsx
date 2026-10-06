// @vitest-environment jsdom
import '@testing-library/jest-dom/vitest';
import './terminal-test-mocks';
import { afterEach, describe, expect, it } from 'vitest';
import { act, cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { failure, success } from '../shared/contracts';
import { App } from './App';
import { deferred, mockApi, session, snapshot } from './test-fixtures';

afterEach(cleanup);
async function setup(initial = snapshot()) {
  const fixture = mockApi(initial); const user = userEvent.setup(); render(<App api={fixture.api} />);
  await screen.findByRole('button', { name: 'Select session Session 1' });
  return { fixture, user };
}
function context(title = 'Session 1') { fireEvent.contextMenu(screen.getByRole('button', { name: `Select session ${title}` })); }

describe('Shellfox session actions', () => {
  it('uses the specified menu order and copies through the preload API with a two-second success toast', async () => {
    const initial = snapshot([session(1, { error: { code: 'FOCUS_DENIED', message: 'Focus denied', retryable: true } })]);
    const { fixture, user } = await setup(initial); context();
    expect(screen.getAllByRole('menuitem').map(item => item.textContent)).toEqual(['Pin to top', 'Rename', 'Environment variables…', 'Open in File Explorer', 'Copy path', 'Clear error', 'Archive']);
    await user.click(screen.getByRole('menuitem', { name: 'Copy path' }));
    expect(fixture.api.copyText).toHaveBeenCalledWith({ text: initial.sessions[0]!.cwd });
    const toast = await screen.findByRole('status'); expect(toast).toHaveTextContent('Path copied');
    expect(toast).toHaveClass('shellfox-toast-info'); expect(screen.queryByRole('alert')).not.toBeInTheDocument();
    await waitFor(() => expect(screen.queryByText('Path copied')).not.toBeInTheDocument(), { timeout: 2600 });
  });
  it('opens only the chosen session folder and shows clipboard/folder failures as toasts', async () => {
    const { fixture, user } = await setup();
    fixture.api.copyText.mockResolvedValueOnce(failure('INTERNAL', 'Clipboard unavailable.'));
    context(); await user.click(screen.getByRole('menuitem', { name: 'Copy path' }));
    expect(await screen.findByRole('alert')).toHaveTextContent('Clipboard unavailable.');
    expect(screen.queryByText('Path copied')).not.toBeInTheDocument();
    await user.click(screen.getByRole('button', { name: 'Dismiss message' }));
    context(); await user.click(screen.getByRole('menuitem', { name: 'Open in File Explorer' }));
    expect(fixture.api.openSessionFolder).toHaveBeenCalledWith({ sessionId: session().id });
    fixture.api.openSessionFolder.mockResolvedValueOnce(failure('NOT_FOUND', 'The session folder no longer exists.'));
    context(); await user.click(screen.getByRole('menuitem', { name: 'Open in File Explorer' }));
    expect(await screen.findByRole('alert')).toHaveTextContent('The session folder no longer exists.');
  });
  it('prefills and focuses the env editor and saves with Ctrl+Enter without changing session selection', async () => {
    const target = session(2, { env: [{ name: 'API_KEY', value: 'sample-key' }, { name: 'SPACES', value: ' padded ' }] });
    const { fixture, user } = await setup(snapshot([session(), target]));
    context('Session 2'); await user.click(screen.getByRole('menuitem', { name: 'Environment variables…' }));
    const modal = screen.getByRole('dialog', { name: 'Environment · Session 2' });
    const input = within(modal).getByRole('textbox', { name: 'Environment variables' });
    expect(input).toHaveValue('API_KEY=sample-key\nSPACES=" padded "'); expect(input).toHaveFocus();
    expect(within(modal).getByText('Applies to new terminals in this session.')).toBeVisible();
    fireEvent.change(input, { target: { value: '# Local\nexport API_KEY="new=key"\nNODE_ENV=development' } });
    await user.keyboard('{Control>}{Enter}{/Control}');
    await waitFor(() => expect(screen.queryByRole('dialog')).not.toBeInTheDocument());
    expect(fixture.api.setSessionEnv).toHaveBeenCalledWith({ sessionId: target.id, env: [{ name: 'API_KEY', value: 'new=key' }, { name: 'NODE_ENV', value: 'development' }] });
    expect(screen.getByRole('button', { name: 'Select session Session 1' })).toHaveAttribute('aria-pressed', 'true');
    expect(fixture.api.closeTab).not.toHaveBeenCalled(); expect(fixture.api.addTab).not.toHaveBeenCalled();
  });
  it('validates live with line numbers, blocks invalid saves, and allows clearing all variables', async () => {
    const { fixture, user } = await setup(snapshot([session(1, { env: [{ name: 'OLD', value: 'value' }] })]));
    context(); await user.click(screen.getByRole('menuitem', { name: 'Environment variables…' }));
    const input = screen.getByRole('textbox', { name: 'Environment variables' });
    fireEvent.change(input, { target: { value: 'PATH=one\npath=two' } });
    expect(input).toHaveAttribute('aria-invalid', 'true'); expect(screen.getByText('Line 2: Duplicate variable name.')).toBeVisible();
    expect(screen.getByRole('button', { name: 'Save' })).toBeDisabled();
    await user.keyboard('{Control>}{Enter}{/Control}'); expect(fixture.api.setSessionEnv).not.toHaveBeenCalled();
    fireEvent.change(input, { target: { value: '\n# No overrides\n' } });
    await user.click(screen.getByRole('button', { name: 'Save' }));
    expect(fixture.api.setSessionEnv).toHaveBeenCalledWith({ sessionId: session().id, env: [] });
  });
  it('blocks unsupported persisted env instead of silently rewriting multiline values', async () => { const { fixture, user } = await setup(snapshot([session(1, { env: [{ name: 'OLD', value: 'first\nOTHER=value' }] })])); context(); await user.click(screen.getByRole('menuitem', { name: 'Environment variables…' })); expect(screen.getByRole('button', { name: 'Save' })).toBeDisabled(); expect(screen.getByText(/Line 1: Values must be single-line/)).toBeVisible(); await user.keyboard('{Control>}{Enter}{/Control}'); expect(fixture.api.setSessionEnv).not.toHaveBeenCalled(); fireEvent.change(screen.getByRole('textbox', { name: 'Environment variables' }), { target: { value: 'OLD=repaired' } }); await user.click(screen.getByRole('button', { name: 'Save' })); expect(fixture.api.setSessionEnv).toHaveBeenCalledWith({ sessionId: session().id, env: [{ name: 'OLD', value: 'repaired' }] }); });
  it('cancels with Escape or Cancel without writing anything', async () => {
    const { fixture, user } = await setup();
    for (const cancel of ['Escape', 'Cancel']) {
      context(); await user.click(screen.getByRole('menuitem', { name: 'Environment variables…' }));
      fireEvent.change(screen.getByRole('textbox', { name: 'Environment variables' }), { target: { value: 'SECRET=unsaved' } });
      if (cancel === 'Escape') await user.keyboard('{Escape}');
      else await user.click(screen.getByRole('button', { name: 'Cancel' }));
      expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
    }
    expect(fixture.api.setSessionEnv).not.toHaveBeenCalled();
  });
  it('locks pending saves and keeps the editor open on backend failure', async () => {
    const { fixture, user } = await setup();
    const pending = deferred<Awaited<ReturnType<typeof fixture.api.setSessionEnv>>>(); fixture.api.setSessionEnv.mockReturnValueOnce(pending.promise);
    context(); await user.click(screen.getByRole('menuitem', { name: 'Environment variables…' }));
    const input = screen.getByRole('textbox', { name: 'Environment variables' }); fireEvent.change(input, { target: { value: 'NAME=value' } });
    await user.keyboard('{Control>}{Enter}{/Control}');
    expect(screen.getByRole('button', { name: 'Save' })).toBeDisabled();
    fireEvent.keyDown(input, { key: 'Enter', ctrlKey: true }); expect(fixture.api.setSessionEnv).toHaveBeenCalledTimes(1);
    await act(async () => pending.resolve(failure('STORAGE_FAILED', 'Could not save environment variables.')));
    expect(screen.getByRole('dialog')).toBeVisible(); expect(input).toHaveValue('NAME=value');
    expect(screen.getByRole('button', { name: 'Save' })).toBeEnabled();
    expect(await screen.findByRole('alert')).toHaveTextContent('Could not save environment variables.');
  });
  it('toasts CLI and Explorer integration failures even after Settings closes', async () => {
    const { fixture, user } = await setup();
    for (const integration of ['cli', 'explorer'] as const) {
      const pending = deferred<Awaited<ReturnType<typeof fixture.api.setCliIntegration>>>();
      const explorerPending = deferred<Awaited<ReturnType<typeof fixture.api.setExplorerIntegration>>>();
      if (integration === 'cli') fixture.api.setCliIntegration.mockReturnValueOnce(pending.promise);
      else fixture.api.setExplorerIntegration.mockReturnValueOnce(explorerPending.promise);
      await user.click(screen.getByRole('button', { name: 'Settings' }));
      await user.click(screen.getByRole('switch', { name: integration === 'cli' ? 'Enable shellfox start <path> in terminals' : 'Add Open in Shellfox to Explorer right-click menu' }));
      await user.keyboard('{Escape}');
      await act(async () => {
        if (integration === 'cli') pending.resolve(failure('STORAGE_FAILED', 'Could not install Shellfox command.'));
        else explorerPending.resolve(failure('STORAGE_FAILED', 'Could not install Explorer action.'));
      });
      expect(await screen.findByRole('alert')).toHaveTextContent(integration === 'cli' ? 'Could not install Shellfox command.' : 'Could not install Explorer action.');
      await user.click(screen.getByRole('button', { name: 'Dismiss message' }));
    }
  });
  it('cannot close a newer editor when an old dismissed save completes', async () => {
    const { fixture, user } = await setup(snapshot([session(), session(2)]));
    const pending = deferred<Awaited<ReturnType<typeof fixture.api.setSessionEnv>>>(); fixture.api.setSessionEnv.mockReturnValueOnce(pending.promise);
    context(); await user.click(screen.getByRole('menuitem', { name: 'Environment variables…' }));
    fireEvent.change(screen.getByRole('textbox'), { target: { value: 'NAME=value' } });
    await user.click(screen.getByRole('button', { name: 'Save' })); await user.keyboard('{Escape}');
    context('Session 2'); await user.click(screen.getByRole('menuitem', { name: 'Environment variables…' }));
    await act(async () => pending.resolve(success(session(1, { env: [{ name: 'NAME', value: 'value' }] }))));
    expect(screen.getByRole('dialog', { name: 'Environment · Session 2' })).toBeVisible();
  });
});
