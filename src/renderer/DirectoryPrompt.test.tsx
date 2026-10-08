// @vitest-environment jsdom
import '@testing-library/jest-dom/vitest';
import { StrictMode } from 'react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { act, cleanup, fireEvent, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import type { ManagerApi } from '../shared/contracts';
import { failure, success } from '../shared/contracts';
import { DirectoryPrompt } from './DirectoryPrompt';
import { deferred, mockApi } from './test-fixtures';

beforeEach(() => {
  // jsdom has no layout. Supply visible bounds so Modal uses its real focus-trap path.
  vi.spyOn(HTMLElement.prototype, 'getClientRects').mockImplementation(() => [new DOMRect(0, 0, 100, 20)] as unknown as DOMRectList);
});
afterEach(() => { cleanup(); vi.useRealTimers(); vi.restoreAllMocks(); });

async function setup(configure?: (api: ReturnType<typeof mockApi>['api']) => void) {
  const { api } = mockApi(); configure?.(api);
  const onCreate = vi.fn<(cwd: string) => Promise<boolean>>(async () => true), onClose = vi.fn();
  const view = render(<DirectoryPrompt api={api} onCreate={onCreate} onClose={onClose} />);
  await act(async () => {});
  const input = screen.getByRole('textbox', { name: 'Folder path' });
  const browse = screen.getByRole('button', { name: 'Browse folders' });
  const form = input.closest('form')!;
  const tab = async () => { await act(async () => { fireEvent.keyDown(input, { key: 'Tab' }); }); };
  const submit = async () => { await act(async () => { fireEvent.submit(form); }); };
  return { api, onCreate, onClose, input, browse, form, tab, submit, ...view };
}

it('opens focused with an empty path, shows home, and does not start the native picker', async () => {
  const f = await setup();
  expect(screen.getByRole('dialog', { name: 'New session' })).toBeVisible();
  expect(f.input).toHaveFocus(); expect(f.input).toHaveValue('');
  expect(screen.getByLabelText('Working directory: C:\\Users\\user')).toHaveAttribute('title', 'C:\\Users\\user');
  expect(f.api.getHomeDirectory).toHaveBeenCalledOnce();
  expect(f.api.chooseDirectory).not.toHaveBeenCalled(); expect(f.api.resolveDirectory).not.toHaveBeenCalled();
});

it.each(['', '~/work space', '"C:\\work space"', '../sibling', String.raw`\\wsl.localhost\Ubuntu\home\tester`])('submits %j unchanged, creates only the resolved cwd, and closes on success', async path => {
  const f = await setup(api => api.resolveDirectory.mockResolvedValueOnce(success({ cwd: 'C:\\resolved folder' })));
  fireEvent.change(f.input, { target: { value: path } });
  await userEvent.setup().keyboard('{Enter}');
  expect(f.api.resolveDirectory).toHaveBeenCalledExactlyOnceWith({ path });
  expect(f.onCreate).toHaveBeenCalledExactlyOnceWith('C:\\resolved folder');
  expect(f.onClose).toHaveBeenCalledOnce(); expect(f.api.chooseDirectory).not.toHaveBeenCalled();
});

it('shows resolve failures inline, retains the draft, and allows an explicit retry', async () => {
  const f = await setup(api => api.resolveDirectory.mockResolvedValueOnce(failure('NOT_FOUND', 'Folder is missing.')));
  fireEvent.change(f.input, { target: { value: 'missing' } }); await f.submit();
  expect(screen.getByRole('alert')).toHaveTextContent('Folder is missing.');
  expect(f.input).toHaveValue('missing'); expect(f.input).toHaveFocus(); expect(f.input).not.toHaveAttribute('readonly');
  expect(f.onCreate).not.toHaveBeenCalled(); expect(f.onClose).not.toHaveBeenCalled();
  expect(f.api.resolveDirectory).toHaveBeenCalledOnce();
  fireEvent.change(f.input, { target: { value: 'fixed' } }); expect(screen.queryByRole('alert')).not.toBeInTheDocument();
  await f.submit(); expect(f.api.resolveDirectory).toHaveBeenCalledTimes(2); expect(f.onClose).toHaveBeenCalledOnce();
});

it('keeps the prompt and restores focus when session creation returns false', async () => {
  const f = await setup(); f.onCreate.mockResolvedValueOnce(false);
  await f.submit();
  expect(f.onCreate).toHaveBeenCalledOnce(); expect(f.onClose).not.toHaveBeenCalled();
  expect(f.input).toHaveFocus(); expect(f.browse).toBeEnabled();
});

it('reports rejected creation callbacks without closing or leaving the prompt locked', async () => {
  const f = await setup(); f.onCreate.mockRejectedValueOnce(new Error('creation failed'));
  await f.submit();
  expect(screen.getByRole('alert')).toHaveTextContent(/could not create.*session/i);
  expect(f.onClose).not.toHaveBeenCalled(); expect(f.browse).toBeEnabled(); expect(f.input).toHaveFocus();
});

it('Browse folders alone starts the picker and creates directly from its validated selection', async () => {
  const f = await setup(api => api.chooseDirectory.mockResolvedValueOnce(success({ cwd: 'C:\\picked folder' })));
  fireEvent.change(f.input, { target: { value: 'typed draft' } });
  await userEvent.setup().click(f.browse);
  expect(f.api.chooseDirectory).toHaveBeenCalledOnce(); expect(f.api.resolveDirectory).not.toHaveBeenCalled();
  expect(f.onCreate).toHaveBeenCalledExactlyOnceWith('C:\\picked folder'); expect(f.onClose).toHaveBeenCalledOnce();
});

it('picker cancellation retains the typed draft, keeps the prompt open, and restores input focus', async () => {
  const f = await setup(api => api.chooseDirectory.mockResolvedValueOnce(success(null)));
  fireEvent.change(f.input, { target: { value: '~/unfinished path' } });
  await userEvent.setup().click(f.browse);
  expect(f.onCreate).not.toHaveBeenCalled(); expect(f.onClose).not.toHaveBeenCalled();
  expect(f.input).toHaveValue('~/unfinished path'); expect(f.input).toHaveFocus(); expect(f.browse).toBeEnabled();
});

it('picker failures are inline and recoverable', async () => {
  const f = await setup(api => api.chooseDirectory.mockResolvedValueOnce(failure('VALIDATION', 'Folder is inaccessible.')));
  await act(async () => { fireEvent.click(f.browse); });
  expect(screen.getByRole('alert')).toHaveTextContent('Folder is inaccessible.'); expect(f.input).toHaveFocus();
  expect(f.onCreate).not.toHaveBeenCalled(); expect(f.onClose).not.toHaveBeenCalled();
});

it('waits beyond the transport deadline for a native picker with no automatic retry', async () => {
  const pick = deferred<Awaited<ReturnType<ManagerApi['chooseDirectory']>>>();
  const f = await setup(api => api.chooseDirectory.mockReturnValueOnce(pick.promise));
  vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout'] });
  await act(async () => { fireEvent.click(f.browse); });
  await act(async () => { vi.advanceTimersByTime(120_000); });
  expect(f.input).toHaveAttribute('readonly'); expect(f.browse).toBeDisabled();
  expect(screen.queryByRole('alert')).not.toBeInTheDocument(); expect(f.api.chooseDirectory).toHaveBeenCalledOnce();
  await act(async () => { pick.resolve(success(null)); });
  expect(f.browse).toBeEnabled(); expect(f.input).toHaveFocus();
});

it('locks duplicate submissions, browse, completion, and dismissal until resolution and creation finish', async () => {
  const resolved = deferred<Awaited<ReturnType<ManagerApi['resolveDirectory']>>>(), created = deferred<boolean>();
  const f = await setup(api => api.resolveDirectory.mockReturnValueOnce(resolved.promise));
  f.onCreate.mockReturnValueOnce(created.promise);
  await f.submit(); await f.submit();
  fireEvent.click(f.browse); await f.tab();
  fireEvent.keyDown(f.input, { key: 'Escape' }); fireEvent.click(screen.getByRole('button', { name: 'Close dialog' }));
  fireEvent.mouseDown(screen.getByRole('dialog').parentElement!);
  expect(f.api.resolveDirectory).toHaveBeenCalledOnce(); expect(f.api.chooseDirectory).not.toHaveBeenCalled();
  expect(f.api.completeDirectory).not.toHaveBeenCalled(); expect(f.onClose).not.toHaveBeenCalled();
  expect(f.input).toHaveAttribute('readonly'); expect(f.browse).toBeDisabled();
  await act(async () => { resolved.resolve(success({ cwd: 'C:\\resolved' })); });
  await f.submit(); expect(f.api.resolveDirectory).toHaveBeenCalledOnce(); expect(f.onCreate).toHaveBeenCalledOnce();
  await act(async () => { created.resolve(false); });
  expect(f.browse).toBeEnabled(); expect(f.input).toHaveFocus();
  fireEvent.keyDown(f.input, { key: 'Escape' }); expect(f.onClose).toHaveBeenCalledOnce();
});

it('turns a rejected backend request into an inline transport error and does not retry', async () => {
  const f = await setup(api => api.resolveDirectory.mockRejectedValueOnce(new Error('bridge disconnected')));
  await f.submit();
  expect(screen.getByRole('alert')).toHaveTextContent('No automatic retry was made.');
  expect(f.api.resolveDirectory).toHaveBeenCalledOnce(); expect(f.onCreate).not.toHaveBeenCalled(); expect(f.browse).toBeEnabled();
});

it('times out noninteractive resolution and ignores its eventual success', async () => {
  const resolved = deferred<Awaited<ReturnType<ManagerApi['resolveDirectory']>>>();
  const f = await setup(api => api.resolveDirectory.mockReturnValueOnce(resolved.promise));
  vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout'] });
  await f.submit(); await act(async () => { vi.advanceTimersByTime(30_000); });
  expect(screen.getByRole('alert')).toHaveTextContent('No automatic retry was made.'); expect(f.browse).toBeEnabled();
  await act(async () => { resolved.resolve(success({ cwd: 'C:\\late' })); });
  expect(f.onCreate).not.toHaveBeenCalled(); expect(f.onClose).not.toHaveBeenCalled();
});

describe('completion', () => {
  it('cycles multiple matches with Tab without moving focus or querying the same prefix again', async () => {
    const f = await setup(api => api.completeDirectory.mockResolvedValueOnce(success({ matches: ['~/alpha/', '~/alpine/'] })));
    fireEvent.change(f.input, { target: { value: '~/al' } });
    await f.tab(); expect(f.input).toHaveValue('~/alpha/'); expect(f.input).toHaveFocus();
    await f.tab(); expect(f.input).toHaveValue('~/alpine/');
    await f.tab(); expect(f.input).toHaveValue('~/alpha/');
    expect(f.api.completeDirectory).toHaveBeenCalledExactlyOnceWith({ path: '~/al' });
  });

  it('descends into a single completed directory on the next Tab', async () => {
    const f = await setup(api => api.completeDirectory
      .mockResolvedValueOnce(success({ matches: ['~/work/'] }))
      .mockResolvedValueOnce(success({ matches: ['~/work/child/'] })));
    fireEvent.change(f.input, { target: { value: '~/wo' } });
    await f.tab(); expect(f.input).toHaveValue('~/work/');
    await f.tab(); expect(f.input).toHaveValue('~/work/child/');
    expect(f.api.completeDirectory).toHaveBeenNthCalledWith(2, { path: '~/work/' });
  });

  it('invalidates the cached cycle when the user edits, and clears completion errors', async () => {
    const f = await setup(api => api.completeDirectory
      .mockResolvedValueOnce(success({ matches: ['alpha/', 'alpine/'] }))
      .mockResolvedValueOnce(failure('NOT_FOUND', 'Cannot list folders.'))
      .mockResolvedValueOnce(success({ matches: ['beta/'] })));
    fireEvent.change(f.input, { target: { value: 'al' } }); await f.tab();
    fireEvent.change(f.input, { target: { value: 'missing/' } }); await f.tab();
    expect(screen.getByRole('alert')).toHaveTextContent('Cannot list folders.'); expect(f.input).toHaveValue('missing/');
    fireEvent.change(f.input, { target: { value: 'be' } }); expect(screen.queryByRole('alert')).not.toBeInTheDocument();
    await f.tab(); expect(f.input).toHaveValue('beta/'); expect(f.api.completeDirectory).toHaveBeenNthCalledWith(3, { path: 'be' });
  });

  it('reports no matches without changing or submitting the input', async () => {
    const f = await setup(api => api.completeDirectory.mockResolvedValueOnce(success({ matches: [] })));
    fireEvent.change(f.input, { target: { value: 'absent' } }); await f.tab();
    expect(screen.getByRole('alert')).toHaveTextContent('No matching folders'); expect(f.input).toHaveValue('absent'); expect(f.input).toHaveFocus();
    expect(f.onCreate).not.toHaveBeenCalled(); expect(f.api.resolveDirectory).not.toHaveBeenCalled();
  });

  it('ignores completion results after an edit and after a newer completion request', async () => {
    const old = deferred<Awaited<ReturnType<ManagerApi['completeDirectory']>>>(), newer = deferred<Awaited<ReturnType<ManagerApi['completeDirectory']>>>();
    const f = await setup(api => api.completeDirectory.mockReturnValueOnce(old.promise).mockReturnValueOnce(newer.promise));
    fireEvent.change(f.input, { target: { value: 'old' } }); await f.tab();
    fireEvent.change(f.input, { target: { value: 'new' } }); await f.tab();
    await act(async () => { newer.resolve(success({ matches: ['new/'] })); });
    await act(async () => { old.resolve(failure('NOT_FOUND', 'Stale completion failure')); });
    expect(f.input).toHaveValue('new/'); expect(screen.queryByRole('alert')).not.toBeInTheDocument();
  });

  it('ignores completion results after submission starts', async () => {
    const pending = deferred<Awaited<ReturnType<ManagerApi['completeDirectory']>>>(), resolved = deferred<Awaited<ReturnType<ManagerApi['resolveDirectory']>>>();
    const f = await setup(api => { api.completeDirectory.mockReturnValueOnce(pending.promise); api.resolveDirectory.mockReturnValueOnce(resolved.promise); });
    fireEvent.change(f.input, { target: { value: 'draft' } }); await f.tab(); await f.submit();
    await act(async () => { pending.resolve(success({ matches: ['stale/'] })); });
    expect(f.input).toHaveValue('draft');
    await act(async () => { resolved.resolve(failure('NOT_FOUND', 'Missing folder')); });
    expect(screen.getByRole('alert')).toHaveTextContent('Missing folder');
  });

  it('Shift+Tab focuses Browse folders without querying or opening it', async () => {
    const f = await setup();
    await userEvent.setup().tab({ shift: true });
    expect(f.browse).toHaveFocus(); expect(f.api.completeDirectory).not.toHaveBeenCalled(); expect(f.api.chooseDirectory).not.toHaveBeenCalled();
  });
});

it('does not overwrite a newer path error with a late home lookup failure', async () => {
  const home = deferred<Awaited<ReturnType<ManagerApi['getHomeDirectory']>>>();
  const f = await setup(api => { api.getHomeDirectory.mockReturnValueOnce(home.promise); api.resolveDirectory.mockResolvedValueOnce(failure('NOT_FOUND', 'Current path is missing.')); });
  fireEvent.change(f.input, { target: { value: 'draft' } }); await f.submit();
  await act(async () => { home.resolve(failure('INTERNAL', 'Stale home error')); });
  expect(screen.getByRole('alert')).toHaveTextContent('Current path is missing.');
});

it('ignores home responses from the discarded StrictMode effect', async () => {
  const { api } = mockApi(), first = deferred<Awaited<ReturnType<ManagerApi['getHomeDirectory']>>>(), second = deferred<Awaited<ReturnType<ManagerApi['getHomeDirectory']>>>();
  api.getHomeDirectory.mockReturnValueOnce(first.promise).mockReturnValueOnce(second.promise);
  render(<StrictMode><DirectoryPrompt api={api} onCreate={vi.fn()} onClose={vi.fn()} /></StrictMode>);
  expect(api.getHomeDirectory).toHaveBeenCalledTimes(2);
  await act(async () => { second.resolve(success({ cwd: 'C:\\current home' })); });
  await act(async () => { first.resolve(success({ cwd: 'C:\\stale home' })); });
  expect(screen.getByLabelText('Working directory: C:\\current home')).toBeVisible();
});

it('shows an initial home lookup failure while leaving typed-path resolution available', async () => {
  const f = await setup(api => api.getHomeDirectory.mockResolvedValueOnce(failure('INTERNAL', 'Home is unavailable.')));
  expect(screen.getByRole('alert')).toHaveTextContent('Home is unavailable.');
  fireEvent.change(f.input, { target: { value: 'C:\\absolute' } }); await f.submit();
  expect(f.api.resolveDirectory).toHaveBeenCalledWith({ path: 'C:\\absolute' }); expect(f.onCreate).toHaveBeenCalledOnce();
});

it.each(['resolve', 'picker'] as const)('does not create from a pending %s response after unmount', async operation => {
  const pending = deferred<Awaited<ReturnType<ManagerApi['chooseDirectory']>>>();
  const f = await setup(api => {
    if (operation === 'picker') api.chooseDirectory.mockReturnValueOnce(pending.promise);
    else api.resolveDirectory.mockReturnValueOnce(pending.promise as Promise<Awaited<ReturnType<ManagerApi['resolveDirectory']>>>);
  });
  if (operation === 'picker') await act(async () => { fireEvent.click(f.browse); }); else await f.submit();
  f.unmount(); await act(async () => { pending.resolve(success({ cwd: 'C:\\late' })); });
  expect(f.onCreate).not.toHaveBeenCalled(); expect(f.onClose).not.toHaveBeenCalled();
});

it('does not close after an in-flight creation completes on an unmounted prompt', async () => {
  const f = await setup(), created = deferred<boolean>(); f.onCreate.mockReturnValueOnce(created.promise);
  await f.submit(); f.unmount(); await act(async () => { created.resolve(true); });
  expect(f.onCreate).toHaveBeenCalledOnce(); expect(f.onClose).not.toHaveBeenCalled();
});

it('can dismiss without creating through Escape, the close button, or the backdrop', async () => {
  const f = await setup();
  fireEvent.keyDown(f.input, { key: 'Escape' }); fireEvent.click(screen.getByRole('button', { name: 'Close dialog' }));
  fireEvent.mouseDown(screen.getByRole('dialog').parentElement!);
  expect(f.onClose).toHaveBeenCalledTimes(3); expect(f.onCreate).not.toHaveBeenCalled(); expect(f.api.chooseDirectory).not.toHaveBeenCalled();
});
