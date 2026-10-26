// @vitest-environment jsdom
import '@testing-library/jest-dom/vitest';
import { StrictMode } from 'react';
import { act, cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { failure, success, type ManagerApi } from '../shared/contracts';
import { DirectoryPrompt } from './DirectoryPrompt';
import { deferred, mockApi } from './test-fixtures';

beforeEach(() => vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout'] }));
afterEach(() => { cleanup(); vi.useRealTimers(); });
async function tick(ms = 120) { await act(async () => { vi.advanceTimersByTime(ms); }); }
async function setup(configure?: (api: ReturnType<typeof mockApi>['api']) => void, remote = false) {
  const { api } = mockApi();
  api.completeDirectory.mockResolvedValue(success({ matches: ['work/', 'workshop/'] }));
  configure?.(api);
  const onCreate = vi.fn(async (_cwd: string) => false), onClose = vi.fn();
  const view = render(<DirectoryPrompt api={api} onCreate={onCreate} onClose={onClose} remote={remote} />);
  await act(async () => {});
  const input = screen.getByRole('combobox', { name: 'Folder path' });
  const edit = (path: string) => fireEvent.change(input, { target: { value: path } });
  const key = (key: string) => fireEvent.keyDown(input, { key });
  const enter = async () => { await act(async () => {
    if (fireEvent.keyDown(input, { key: 'Enter' })) fireEvent.submit(input.closest('form')!);
  }); };
  const tab = async () => { await act(async () => { key('Tab'); }); };
  return { ...view, api, onCreate, onClose, input, edit, key, enter, tab };
}

it('lists the current location after a debounce without changing the draft', async () => {
  const f = await setup();
  await tick(119); expect(f.api.completeDirectory).not.toHaveBeenCalled();
  await tick(1);
  expect(f.api.completeDirectory).toHaveBeenCalledExactlyOnceWith({ path: '' });
  expect(screen.getAllByRole('option').map(node => node.textContent)).toEqual(['work/', 'workshop/']);
  expect(f.input).toHaveValue(''); expect(f.input).toHaveFocus();
  expect(f.input).toHaveAttribute('aria-expanded', 'true');
  expect(f.input).toHaveAttribute('aria-controls', screen.getByRole('listbox').id);
  expect(f.input).toHaveAttribute('aria-activedescendant', screen.getByRole('option', { selected: true }).id);
  expect(f.api.resolveDirectory).not.toHaveBeenCalled();
});

it('debounces edits and queries only the latest prefix, including quotes and relative paths', async () => {
  const f = await setup(api => api.completeDirectory.mockImplementation(async ({ path }) => success({ matches: path === '"../work space/a"' ? ['../work space/alpha/'] : [] })));
  f.edit('../w'); await tick(80); f.edit('../work space/'); await tick(80); f.edit('"../work space/a"');
  await tick(119); expect(f.api.completeDirectory).not.toHaveBeenCalled();
  await tick(1);
  expect(f.api.completeDirectory).toHaveBeenCalledExactlyOnceWith({ path: '"../work space/a"' });
  expect(screen.getByRole('option')).toHaveTextContent('../work space/alpha/');
  expect(f.input).toHaveValue('"../work space/a"');
});

it('keeps work and workshop visible without appending a slash, then lists children on a typed slash', async () => {
  const f = await setup(api => api.completeDirectory.mockImplementation(async ({ path }) => success({ matches: path.endsWith('/') ? ['~/work/child/'] : ['~/work/', '~/workshop/'] })));
  f.edit('~/work'); await tick();
  expect(screen.getAllByRole('option')).toHaveLength(2); expect(f.input).toHaveValue('~/work');
  f.edit('~/work/'); await tick();
  expect(screen.getByRole('option')).toHaveTextContent('~/work/child/');
  expect(f.api.completeDirectory).toHaveBeenLastCalledWith({ path: '~/work/' });
});

it.each([
  ['~/work', '~/work/'], ['~/work/', '~/work/'], ['"~/work"', '~/work/'],
  ['"C:\\work space"', 'C:\\work space\\'], ['~\\work', '~\\work\\'],
])('hides only a sole exact match for %j without modifying input', async (path, match) => {
  const f = await setup(api => api.completeDirectory.mockResolvedValue(success({ matches: [match] })));
  f.edit(path); await tick();
  expect(screen.queryByRole('listbox')).not.toBeInTheDocument();
  expect(f.input).toHaveAttribute('aria-expanded', 'false'); expect(f.input).toHaveValue(path);
  expect(f.input).not.toHaveAttribute('aria-activedescendant');
});

it('arrows wrap without editing; Enter accepts only, and a second Enter submits', async () => {
  const f = await setup(); f.edit('wo'); await tick();
  f.key('ArrowUp'); expect(screen.getByRole('option', { selected: true })).toHaveTextContent('workshop/');
  f.key('ArrowDown'); expect(screen.getByRole('option', { selected: true })).toHaveTextContent('work/');
  f.key('ArrowDown'); expect(f.input).toHaveValue('wo'); expect(f.input).toHaveFocus();
  await f.enter();
  expect(f.input).toHaveValue('workshop/'); expect(f.api.resolveDirectory).not.toHaveBeenCalled();
  expect(f.onCreate).not.toHaveBeenCalled(); expect(screen.getAllByRole('option')).toHaveLength(2);
  await tick(500); expect(f.api.completeDirectory).toHaveBeenCalledOnce();
  await f.enter();
  expect(f.api.resolveDirectory).toHaveBeenCalledExactlyOnceWith({ path: 'workshop/' }); expect(f.onCreate).toHaveBeenCalledOnce();
});

it('accepts the highlighted first match on Enter without submitting the form', async () => {
  const f = await setup(); await tick();
  await f.enter();
  expect(f.input).toHaveValue('work/'); expect(f.api.resolveDirectory).not.toHaveBeenCalled();
});

it('reuses live results for Tab cycling without fetching children automatically', async () => {
  const f = await setup(); f.edit('wo'); await tick();
  await f.tab(); expect(f.input).toHaveValue('work/');
  await tick(); await f.tab(); expect(f.input).toHaveValue('workshop/');
  await f.tab(); expect(f.input).toHaveValue('work/');
  expect(f.api.completeDirectory).toHaveBeenCalledExactlyOnceWith({ path: 'wo' }); expect(f.input).toHaveFocus();
});

it('hides an accepted sole candidate until further typing or an explicit Tab descends', async () => {
  const f = await setup(api => api.completeDirectory.mockImplementation(async ({ path }) => success({ matches: path === 'wo' ? ['work/'] : ['work/child/'] })));
  f.edit('wo'); await tick(); await f.enter();
  expect(f.input).toHaveValue('work/'); expect(screen.queryByRole('listbox')).not.toBeInTheDocument();
  await tick(500); expect(f.api.completeDirectory).toHaveBeenCalledOnce();
  await f.tab(); expect(f.input).toHaveValue('work/child/');
  expect(f.api.completeDirectory).toHaveBeenLastCalledWith({ path: 'work/' });
  f.edit('work/child/n'); await tick(); expect(f.api.completeDirectory).toHaveBeenLastCalledWith({ path: 'work/child/n' });
});

it('second Enter opens the accepted directory even when its child completions are nonempty', async () => {
  const f = await setup(api => api.completeDirectory.mockImplementation(async ({ path }) => success({
    matches: path === '~/work/' ? ['~/work/child/', '~/work/other/'] : ['~/work/'],
  })));
  f.edit('~/wo'); await tick(); await f.enter();
  expect(f.input).toHaveValue('~/work/'); expect(f.api.resolveDirectory).not.toHaveBeenCalled();
  await tick(500);
  expect(f.api.completeDirectory).toHaveBeenCalledExactlyOnceWith({ path: '~/wo' });
  await f.enter();
  expect(f.api.resolveDirectory).toHaveBeenCalledExactlyOnceWith({ path: '~/work/' });
  expect(f.onCreate).toHaveBeenCalledOnce();
});

it('arrows explicitly re-arm candidate acceptance after accepting a sibling', async () => {
  const f = await setup(); f.edit('wo'); await tick(); await f.enter();
  expect(f.input).toHaveValue('work/');
  f.key('ArrowDown'); await f.enter();
  expect(f.input).toHaveValue('workshop/'); expect(f.api.resolveDirectory).not.toHaveBeenCalled();
  await f.enter(); expect(f.api.resolveDirectory).toHaveBeenCalledWith({ path: 'workshop/' });
});

it('click accepts only and retains input focus, with Enter still required to open', async () => {
  const f = await setup(); f.edit('wo'); await tick();
  fireEvent.mouseDown(screen.getByRole('option', { name: 'workshop/' }));
  fireEvent.click(screen.getByRole('option', { name: 'workshop/' }));
  expect(f.input).toHaveValue('workshop/'); expect(f.input).toHaveFocus(); expect(f.api.resolveDirectory).not.toHaveBeenCalled();
  await f.enter(); expect(f.api.resolveDirectory).toHaveBeenCalledWith({ path: 'workshop/' });
});

it('ignores a stale response immediately after editing, even before the next debounce fires', async () => {
  const old = deferred<Awaited<ReturnType<ManagerApi['completeDirectory']>>>();
  const f = await setup(api => api.completeDirectory.mockReturnValueOnce(old.promise).mockResolvedValue(success({ matches: ['new/'] })));
  f.edit('old'); await tick(); f.edit('ne');
  await act(async () => { old.resolve(success({ matches: ['old/'] })); });
  expect(screen.queryByRole('listbox')).not.toBeInTheDocument(); expect(f.input).toHaveValue('ne');
  await tick(); expect(screen.getByRole('option')).toHaveTextContent('new/');
});

it('does not replace newer results or errors with a stale completion failure', async () => {
  const old = deferred<Awaited<ReturnType<ManagerApi['completeDirectory']>>>();
  const f = await setup(api => api.completeDirectory.mockReturnValueOnce(old.promise));
  f.edit('old'); await tick(); f.edit('wo'); await tick();
  await act(async () => { old.resolve(failure('INTERNAL', 'Stale listing error')); });
  expect(screen.getAllByRole('option')).toHaveLength(2); expect(screen.queryByRole('alert')).not.toBeInTheDocument();
});

it('keeps live no-match and inaccessible-parent results quiet; explicit Tab reports errors', async () => {
  const f = await setup(api => api.completeDirectory.mockResolvedValue(failure('NOT_FOUND', 'Cannot list folders.')));
  f.edit('new/path'); await tick();
  expect(screen.queryByRole('listbox')).not.toBeInTheDocument(); expect(screen.queryByRole('alert')).not.toBeInTheDocument();
  await f.tab(); expect(screen.getByRole('alert')).toHaveTextContent('Cannot list folders.');
  f.api.completeDirectory.mockResolvedValue(success({ matches: [] })); f.edit('absent'); await tick();
  expect(screen.queryByRole('alert')).not.toBeInTheDocument();
  await f.enter(); expect(f.api.resolveDirectory).toHaveBeenCalledWith({ path: 'absent' });
});

it('preserves IME composition and never queries, accepts, submits or dismisses during it', async () => {
  const f = await setup(); await tick(); f.api.completeDirectory.mockClear();
  fireEvent.compositionStart(f.input); f.edit('作');
  f.key('ArrowDown'); f.key('Tab'); f.key('Enter'); f.key('Escape');
  fireEvent.submit(f.input.closest('form')!); await tick(500);
  expect(f.input).toHaveValue('作'); expect(f.api.completeDirectory).not.toHaveBeenCalled();
  expect(f.api.resolveDirectory).not.toHaveBeenCalled(); expect(f.onClose).not.toHaveBeenCalled();
  expect(screen.queryByRole('listbox')).not.toBeInTheDocument();
  f.edit('作業'); fireEvent.compositionEnd(f.input); await tick();
  expect(f.api.completeDirectory).toHaveBeenCalledExactlyOnceWith({ path: '作業' });
});

it.each([{ isComposing: true }, { keyCode: 229 }])('guards native IME Enter even without composition events: %j', async native => {
  const f = await setup(); await tick();
  const allowed = fireEvent.keyDown(f.input, { key: 'Enter', ...native });
  expect(allowed).toBe(false); expect(f.input).toHaveValue(''); expect(f.api.resolveDirectory).not.toHaveBeenCalled();
});

it('invalidates pending results on blur and unmount, and reloads when focus returns', async () => {
  const pending = deferred<Awaited<ReturnType<ManagerApi['completeDirectory']>>>();
  const f = await setup(api => api.completeDirectory.mockReturnValueOnce(pending.promise)); await tick();
  fireEvent.blur(f.input);
  await act(async () => { pending.resolve(success({ matches: ['stale/'] })); });
  expect(screen.queryByRole('listbox')).not.toBeInTheDocument();
  fireEvent.focus(f.input); await tick(); expect(screen.getAllByRole('option')).toHaveLength(2);
  f.edit('later'); f.unmount(); await tick(500); expect(f.api.completeDirectory).toHaveBeenCalledTimes(2);
});

it('supports remote hosts without a native picker and without requiring new backend methods', async () => {
  const f = await setup(undefined, true); await tick();
  expect(screen.queryByRole('button', { name: 'Browse folders' })).not.toBeInTheDocument();
  f.key('ArrowDown'); await f.enter();
  expect(f.input).toHaveValue('workshop/'); expect(f.api.resolveDirectory).not.toHaveBeenCalled();
});

it('runs only one live request after StrictMode discards its initial effect', async () => {
  const { api } = mockApi();
  render(<StrictMode><DirectoryPrompt api={api} onCreate={vi.fn()} onClose={vi.fn()} /></StrictMode>);
  await tick(); expect(api.completeDirectory).toHaveBeenCalledOnce();
});
