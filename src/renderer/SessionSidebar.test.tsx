// @vitest-environment jsdom
import '@testing-library/jest-dom/vitest';
import { useStore } from 'zustand';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { act, cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { failure, success } from '../shared/contracts';
import { SessionSidebar, shortenHome } from './SessionSidebar';
import { createManagerClient } from './store';
import type { ManagerClient } from './store';
import { deferred, mockApi, session, snapshot } from './test-fixtures';

const clients: ManagerClient[] = [];
afterEach(() => { cleanup(); clients.splice(0).forEach(client => client.stop()); vi.restoreAllMocks(); vi.unstubAllGlobals(); });
const archived = (id: number, title: string) => session(id, { title, status: 'settled', settledAt: '2026-10-04T11:00:00.000Z' });
function Navigation({ client }: { client: ManagerClient }) {
  const state = useStore(client.store);
  return <SessionSidebar client={client} sessions={state.snapshot?.sessions ?? []} selectedId={state.selectedId} busy={state.busyTabIds} invalidation={state.historyStatusVersion} archiveInvalidation={state.archiveVersion} pageSize={1} />;
}
const flush = async () => { for (let i = 0; i < 8; i++) await Promise.resolve(); };

describe('sidebar cwd display', () => {
  it.each([
    [String.raw`\\wsl.localhost\Debian\var\www`, '/var/www'],
    [String.raw`\\wsl$\Debian\var\www`, '/var/www'],
    [String.raw`\\wsl$\Debian`, '/'],
    [String.raw`\\wsl$\Debian\home\user\work`, '~/work'],
    [String.raw`C:\Users\user\work`, String.raw`~\work`],
    [String.raw`C:\work`, String.raw`C:\work`],
    [String.raw`\\server\share\work`, String.raw`\\server\share\work`],
    ['/var/www', '/var/www'], ['/home/user/work', '~/work'],
  ])('formats %s as %s', (cwd, expected) => expect(shortenHome(cwd)).toBe(expected));

  it('displays the guest path but keeps the original UNC tooltip and copy path', async () => {
    const cwd = String.raw`\\wsl.localhost\Debian\var\www`;
    const fixture = mockApi(snapshot([session(1, { cwd })]));
    const client = createManagerClient(fixture.api); clients.push(client); client.start();
    const user = userEvent.setup(); render(<Navigation client={client} />);
    expect(await screen.findByText('/var/www')).toHaveAttribute('title', cwd);
    await user.pointer({ keys: '[MouseRight]', target: screen.getByRole('button', { name: 'Select session Session 1' }) });
    await user.click(screen.getByRole('menuitem', { name: 'Copy path' }));
    expect(fixture.api.copyText).toHaveBeenCalledWith({ text: cwd });
  });
});

describe('archive refresh and pagination', () => {
  it('keeps the loaded page range through native polls, archive and restore events', async () => {
    const initial = snapshot(); const fixture = mockApi(initial);
    let items = [archived(20, 'Archive one'), archived(21, 'Archive two')];
    fixture.api.getHistory.mockImplementation(async query => success({ items: items.slice((query.page - 1) * query.pageSize, query.page * query.pageSize), total: items.length, page: query.page, pageSize: query.pageSize }));
    const client = createManagerClient(fixture.api); clients.push(client); client.start();
    const user = userEvent.setup(); render(<Navigation client={client} />);
    await screen.findByRole('button', { name: 'Archived · 2' });
    expect(fixture.api.getHistory).toHaveBeenCalledTimes(1);
    expect(fixture.api.getHistory.mock.calls[0]![0].pageSize).toBe(1);
    for (const revision of [2, 3, 4]) await act(async () => { fixture.emit({ ...initial, revision }, 'native'); await flush(); });
    expect(fixture.api.getHistory).toHaveBeenCalledTimes(1);

    await user.click(screen.getByRole('button', { name: 'Archived · 2' }));
    await screen.findByRole('button', { name: 'Select session Archive one' });
    await waitFor(() => expect(screen.getByRole('button', { name: 'More' })).toBeEnabled());
    await user.click(screen.getByRole('button', { name: 'More' }));
    await screen.findByRole('button', { name: 'Select session Archive two' });
    items = [items[0]!, { ...items[1]!, error: { code: 'MONITOR_UNAVAILABLE', message: 'Archived agent error', retryable: true } }];
    for (const revision of [5, 6, 7]) {
      await act(async () => { fixture.emit({ ...initial, revision }, 'native'); await flush(); });
      expect(screen.getByRole('button', { name: 'Select session Archive two' })).toBeVisible();
    }
    expect(await screen.findByLabelText('Error: Archived agent error')).toBeVisible();
    expect(fixture.api.getHistory.mock.calls.slice(-2).map(([query]) => query.page)).toEqual([1, 2]);

    items.push(archived(22, 'Archive three'));
    await act(async () => { fixture.emit({ ...initial, revision: 8 }, 'history'); await flush(); });
    await screen.findByRole('button', { name: 'Archived · 3' });
    expect(screen.getByRole('button', { name: 'Select session Archive two' })).toBeVisible();
    expect(fixture.api.getHistory.mock.calls.slice(-2).map(([query]) => query.page)).toEqual([1, 2]);
    items = items.slice(1); // Restore the first archived session.
    await act(async () => { fixture.emit({ ...initial, revision: 9 }, 'history'); await flush(); });
    expect(await screen.findByRole('button', { name: 'Select session Archive three' })).toBeVisible();
    expect(screen.queryByRole('button', { name: 'Select session Archive one' })).not.toBeInTheDocument();

    await user.click(screen.getByRole('button', { name: 'Archived · 2' }));
    await waitFor(() => expect(fixture.api.getHistory.mock.calls.at(-1)![0].pageSize).toBe(1));
    const beforePolls = fixture.api.getHistory.mock.calls.length;
    for (const revision of [10, 11, 12]) await act(async () => { fixture.emit({ ...initial, revision }, 'native'); await flush(); });
    expect(fixture.api.getHistory).toHaveBeenCalledTimes(beforePolls);
    items.push(archived(23, 'Archive four'));
    await act(async () => { fixture.emit({ ...initial, revision: 13 }, 'history'); await flush(); });
    await screen.findByRole('button', { name: 'Archived · 3' });
    expect(fixture.api.getHistory.mock.calls.at(-1)![0]).toMatchObject({ page: 1, pageSize: 1 });
    await user.click(screen.getByRole('button', { name: 'Archived · 3' }));
    expect(await screen.findByRole('button', { name: 'Select session Archive three' })).toBeVisible();
    expect(fixture.api.getHistory.mock.calls.slice(-2).map(([query]) => query.page)).toEqual([1, 2]);
  });
});

describe('session activation and pinning', () => {
  const closedTabs = (index: number, title: string) => { const item = session(index, { title }); for (const tab of item.tabs) tab.lifecycle = 'closed'; return item; };
  async function setup(sessions: ReturnType<typeof session>[]) {
    const fixture = mockApi(snapshot(sessions));
    const client = createManagerClient(fixture.api); clients.push(client); client.start();
    const user = userEvent.setup(); render(<Navigation client={client} />);
    await screen.findByRole('button', { name: `Select session ${sessions[0]!.title}` });
    return { fixture, client, user };
  }
  it('opens one fresh terminal when selecting a saved session without open terminals (after restart)', async () => {
    const { fixture, client, user } = await setup([closedTabs(1, 'Restored')]);
    const reopened = session(1, { title: 'Restored' });
    // Like the backend: activation opens a shell and publishes a new snapshot.
    fixture.api.activateSession.mockImplementationOnce(async () => { fixture.emit({ ...snapshot([reopened]), revision: 100 }); return success(reopened); });
    await user.click(screen.getByRole('button', { name: 'Select session Restored' }));
    await waitFor(() => expect(fixture.api.activateSession).toHaveBeenCalledWith({ sessionId: reopened.id }));
    await waitFor(() => expect(client.store.getState().activeTabIds[reopened.id]).toBe(reopened.tabs[0]!.id));
  });
  it('does not open another terminal for sessions that already have one, or for archived sessions', async () => {
    const { fixture, user } = await setup([session(1, { title: 'Live' })]);
    await user.click(screen.getByRole('button', { name: 'Select session Live' }));
    await flush();
    expect(fixture.api.activateSession).not.toHaveBeenCalled();
  });
  it('pins and unpins from the context menu, showing a pin marker', async () => {
    const { fixture, user } = await setup([session(1, { title: 'One' }), session(2, { title: 'Two' })]);
    await user.pointer({ keys: '[MouseRight]', target: screen.getByRole('button', { name: 'Select session Two' }) });
    await user.click(screen.getByRole('menuitem', { name: 'Pin to top' }));
    expect(fixture.api.setSessionPinned).toHaveBeenCalledWith({ sessionId: session(2).id, pinned: true });
    await waitFor(() => expect(screen.getByLabelText('Pinned')).toBeInTheDocument());
    await user.pointer({ keys: '[MouseRight]', target: screen.getByRole('button', { name: 'Select session Two' }) });
    await user.click(screen.getByRole('menuitem', { name: 'Unpin' }));
    expect(fixture.api.setSessionPinned).toHaveBeenLastCalledWith({ sessionId: session(2).id, pinned: false });
    await waitFor(() => expect(screen.queryByLabelText('Pinned')).not.toBeInTheDocument());
  });
});

describe('permanent deletion of archived sessions', () => {
  async function setup(items: ReturnType<typeof archived>[], live = [session(1, { title: 'Live' })]) {
    const fixture = mockApi(snapshot(live));
    fixture.api.getHistory.mockImplementation(async query => success({ items, total: items.length, page: query.page, pageSize: query.pageSize }));
    const client = createManagerClient(fixture.api); clients.push(client); client.start();
    const user = userEvent.setup(); render(<Navigation client={client} />);
    await user.click(await screen.findByRole('button', { name: `Archived · ${items.length}` }));
    await screen.findByRole('button', { name: `Select session ${items[0]!.title}` });
    return { fixture, client, user };
  }
  it('offers deletion only for archived sessions', async () => {
    const { user } = await setup([archived(20, 'Old')]);
    await user.pointer({ keys: '[MouseRight]', target: screen.getByRole('button', { name: 'Select session Live' }) });
    expect(screen.queryByRole('menuitem', { name: 'Delete permanently…' })).not.toBeInTheDocument();
    await user.keyboard('{Escape}');
    await user.pointer({ keys: '[MouseRight]', target: screen.getByRole('button', { name: 'Select session Old' }) });
    expect(screen.getByRole('menuitem', { name: 'Delete permanently…' })).toBeVisible();
  });
  it('only mentions closing terminals when the archived session still has a running one', async () => {
    const old = archived(20, 'Old'), done = archived(21, 'Done');
    for (const tab of done.tabs) tab.lifecycle = 'closed';
    const { user } = await setup([old, done]);
    await user.pointer({ keys: '[MouseRight]', target: screen.getByRole('button', { name: 'Select session Done' }) });
    await user.click(screen.getByRole('menuitem', { name: 'Delete permanently…' }));
    expect(await screen.findByRole('dialog', { name: 'Delete session' })).toHaveTextContent('Permanently delete “Done”? This cannot be undone.');
  });
  it('requires confirmation, can be cancelled, then deletes and refreshes the archive', async () => {
    const items = [archived(20, 'Old')];
    const { fixture, client, user } = await setup(items);
    await user.pointer({ keys: '[MouseRight]', target: screen.getByRole('button', { name: 'Select session Old' }) });
    await user.click(screen.getByRole('menuitem', { name: 'Delete permanently…' }));
    const dialog = await screen.findByRole('dialog', { name: 'Delete session' });
    expect(dialog).toHaveTextContent('Permanently delete “Old”? Its running terminals will be closed. This cannot be undone.');
    expect(fixture.api.deleteSession).not.toHaveBeenCalled();
    await user.click(screen.getByRole('button', { name: 'Cancel' }));
    expect(screen.queryByRole('dialog', { name: 'Delete session' })).not.toBeInTheDocument();
    expect(fixture.api.deleteSession).not.toHaveBeenCalled();

    await user.pointer({ keys: '[MouseRight]', target: screen.getByRole('button', { name: 'Select session Old' }) });
    await user.click(screen.getByRole('menuitem', { name: 'Delete permanently…' }));
    client.select(items[0]!);
    fixture.api.deleteSession.mockImplementationOnce(async () => { items.splice(0); return success({ deleted: true }); });
    await user.click(await screen.findByRole('button', { name: 'Delete' }));
    expect(fixture.api.deleteSession).toHaveBeenCalledExactlyOnceWith({ sessionId: session(20).id });
    await waitFor(() => expect(screen.queryByRole('dialog', { name: 'Delete session' })).not.toBeInTheDocument());
    await screen.findByRole('button', { name: 'Archived · 0' });
    expect(screen.queryByRole('button', { name: 'Select session Old' })).not.toBeInTheDocument();
    expect(client.store.getState().historical).toBeNull();
  });
  it('keeps the session and the confirmation open when deletion fails', async () => {
    const { fixture, user } = await setup([archived(20, 'Old')]);
    fixture.api.deleteSession.mockResolvedValueOnce(failure('UNSUPPORTED', 'Close terminals first.'));
    await user.pointer({ keys: '[MouseRight]', target: screen.getByRole('button', { name: 'Select session Old' }) });
    await user.click(screen.getByRole('menuitem', { name: 'Delete permanently…' }));
    await user.click(await screen.findByRole('button', { name: 'Delete' }));
    await waitFor(() => expect(fixture.api.deleteSession).toHaveBeenCalledTimes(1));
    expect(await screen.findByRole('dialog', { name: 'Delete session' })).toBeVisible();
    expect(screen.getByRole('button', { name: 'Select session Old' })).toBeVisible();
  });
});

// jsdom has no layout. Model the existing single scrollbar, 36px rows and
// archive header so these tests exercise offsets rather than a zero-sized DOM.
function sidebarLayout() {
  let height = 180;
  const observers = new Set<() => void>();
  vi.stubGlobal('ResizeObserver', class {
    constructor(private callback: () => void) { observers.add(callback); }
    observe() {}
    disconnect() { observers.delete(this.callback); }
  });
  vi.spyOn(HTMLElement.prototype, 'clientHeight', 'get').mockImplementation(function (this: HTMLElement) {
    return this.classList.contains('sidebar-scroll') ? height : 0;
  });
  const original = HTMLElement.prototype.getBoundingClientRect;
  vi.spyOn(HTMLElement.prototype, 'getBoundingClientRect').mockImplementation(function (this: HTMLElement) {
    if (this.getAttribute('role') !== 'list') return original.call(this);
    const scroll = this.closest('.sidebar-scroll')!;
    const live = scroll.querySelector('[aria-label="Sessions"] [role="listitem"]');
    const liveCount = Number(live?.getAttribute('aria-setsize') ?? 0);
    const archivedList = this.parentElement?.getAttribute('aria-label') === 'Archived sessions';
    const top = 4 + (archivedList ? liveCount * 36 + 32 : 0) - scroll.scrollTop;
    return { top, bottom: top + this.children.length * 36, left: 0, right: 220, width: 220, height: 180, x: 0, y: top, toJSON() {} };
  });
  return { resize: (value: number) => { height = value; observers.forEach(callback => callback()); } };
}

function largeSidebar(count = 1000) {
  sidebarLayout();
  const items = Array.from({ length: count }, (_, index) => session(index + 1));
  const fixture = mockApi(snapshot(items));
  const client = createManagerClient(fixture.api); clients.push(client);
  const props = { client, sessions: items, selectedId: null as string | null, busy: {}, invalidation: 0, pageSize: 120 };
  const view = render(<SessionSidebar {...props} />);
  const scroll = view.container.querySelector<HTMLDivElement>('.sidebar-scroll')!;
  const scrollTo = async (top: number) => {
    scroll.scrollTop = top;
    fireEvent.scroll(scroll);
    await waitFor(() => {
      const index = Math.max(1, Math.min(count, Math.floor((top - 4) / 36) + 1));
      expect(screen.getByRole('button', { name: `Select session Session ${index}` })).toBeInTheDocument();
    });
  };
  return { ...view, fixture, client, props, items, scroll, scrollTo, user: userEvent.setup() };
}

describe('large session lists', () => {
  it('bounds mounted rows, keeps total height and list positions, and renders fresh status after scrolling', async () => {
    const { container, props, rerender, items, scrollTo } = largeSidebar(2000);
    const list = screen.getByRole('region', { name: 'Sessions' }).querySelector('[role="list"]')!;
    expect(container.querySelectorAll('.session-row').length).toBeLessThan(30);
    expect(screen.queryByRole('button', { name: 'Select session Session 1001' })).not.toBeInTheDocument();
    const fullHeight = Array.from(list.children).reduce((sum, child) => sum + (child.getAttribute('role') === 'listitem' ? 36 : Number.parseFloat((child as HTMLElement).style.height)), 0);
    expect(fullHeight).toBe(2000 * 36);

    const updated = items.map(item => item.id === items[1000]!.id ? { ...item, error: { code: 'INTERNAL' as const, message: 'New error', retryable: true } } : item);
    rerender(<SessionSidebar {...props} sessions={updated} />);
    await scrollTo(1000 * 36 + 4);
    const target = screen.getByRole('button', { name: 'Select session Session 1001' }).closest('[role="listitem"]')!;
    expect(target).toHaveAttribute('aria-posinset', '1001');
    expect(target).toHaveAttribute('aria-setsize', '2000');
    expect(within(target as HTMLElement).getByLabelText('Error: New error')).toBeInTheDocument();
    expect(container.querySelectorAll('.session-row').length).toBeLessThan(30);
  });

  it('reveals a selection on first mount after the parent scroll ref attaches', () => {
    sidebarLayout();
    const items = Array.from({ length: 1000 }, (_, index) => session(index + 1));
    const client = createManagerClient(mockApi().api); clients.push(client);
    const { container } = render(<SessionSidebar client={client} sessions={items} selectedId={items[799]!.id} busy={{}} invalidation={0} pageSize={120} />);
    expect(screen.getByRole('button', { name: 'Select session Session 800' })).toHaveAttribute('aria-pressed', 'true');
    expect(container.querySelector('.sidebar-scroll')!.scrollTop).toBeGreaterThan(700 * 36);
  });

  it('keeps a focused row mounted during a mouse scroll, then releases it when focus leaves', async () => {
    const { scrollTo, user } = largeSidebar();
    const target = screen.getByRole('button', { name: 'Select session Session 6' });
    act(() => target.focus());
    await scrollTo(500 * 36 + 4);
    expect(target).toHaveFocus();
    await user.click(screen.getByRole('button', { name: 'Archived · 0' }));
    expect(target).not.toBeInTheDocument();
  });

  it('reveals external selection once, without snapping back on activity updates or a pin reorder', async () => {
    const { props, rerender, items, scroll, scrollTo, user, client } = largeSidebar();
    const selectedId = items[799]!.id;
    rerender(<SessionSidebar {...props} selectedId={selectedId} />);
    expect(screen.getByRole('button', { name: 'Select session Session 800' })).toHaveAttribute('aria-pressed', 'true');
    expect(scroll.scrollTop).toBeGreaterThan(700 * 36);
    await scrollTo(4);
    rerender(<SessionSidebar {...props} selectedId={selectedId} busy={{ [items[799]!.tabs[0]!.id]: true }} sessions={[items[799]!, ...items.filter(item => item.id !== selectedId)]} />);
    expect(scroll.scrollTop).toBe(4);
    await user.click(screen.getByRole('button', { name: 'Select session Session 2' }));
    expect(client.store.getState().selectedId).toBe(items[1]!.id);
  });

  it('tabs across window boundaries and supports Home/End without trapping focus', async () => {
    const { user, container } = largeSidebar();
    const first = screen.getByRole('button', { name: 'Select session Session 1' });
    act(() => first.focus());
    for (let index = 0; index < 15; index++) await user.tab();
    expect(screen.getByRole('button', { name: 'Select session Session 16' })).toHaveFocus();
    expect(container.querySelectorAll('.session-row').length).toBeLessThan(35);
    await user.keyboard('{End}');
    expect(screen.getByRole('button', { name: 'Select session Session 1000' })).toHaveFocus();
    await user.tab();
    expect(screen.getByRole('button', { name: 'Archived · 0' })).toHaveFocus();
    await user.tab({ shift: true });
    expect(screen.getByRole('button', { name: 'Select session Session 1000' })).toHaveFocus();
    await user.keyboard('{Home}');
    expect(first).toHaveFocus();
    await user.tab();
    await user.tab({ shift: true });
    expect(first).toHaveFocus();
  });

  it('retains context-menu and rename targets while scrolling, including keyboard menu access', async () => {
    const { user, scrollTo, fixture } = largeSidebar();
    const target = screen.getByRole('button', { name: 'Select session Session 6' });
    act(() => target.focus());
    await user.keyboard('{Shift>}{F10}{/Shift}');
    expect(screen.getByRole('menu')).toBeVisible();
    await scrollTo(500 * 36 + 4);
    expect(target).toBeInTheDocument();
    await user.keyboard('{Escape}');
    expect(target).toHaveFocus();
    await user.pointer({ keys: '[MouseRight]', target });
    await user.click(screen.getByRole('menuitem', { name: 'Rename' }));
    const input = screen.getByRole('textbox', { name: 'Session title' });
    await user.clear(input);
    await user.type(input, 'Kept name');
    await scrollTo(700 * 36 + 4);
    expect(screen.getByRole('textbox', { name: 'Session title' })).toBe(input);
    expect(input).toHaveValue('Kept name');
    await user.keyboard('{Enter}');
    await waitFor(() => expect(fixture.api.renameSession).toHaveBeenCalledWith({ sessionId: session(6).id, title: 'Kept name' }));
    expect(screen.getByRole('button', { name: 'Select session Session 6' })).toHaveFocus();
  });

  it('does not unmount a drag source or cancel bubbled drag/drop events', async () => {
    const { container, scrollTo } = largeSidebar();
    const source = screen.getByRole('button', { name: 'Select session Session 6' });
    const drop = vi.fn();
    container.addEventListener('drop', drop);
    fireEvent.dragStart(source);
    await scrollTo(500 * 36 + 4);
    expect(source).toBeInTheDocument();
    expect(fireEvent.drop(source)).toBe(true);
    expect(drop).toHaveBeenCalledOnce();
    fireEvent.dragEnd(source);
    await waitFor(() => expect(source).not.toBeInTheDocument());
  });

  it('returns focus to the row when cancelling rename', async () => {
    const { user, scrollTo } = largeSidebar();
    const target = screen.getByRole('button', { name: 'Select session Session 6' });
    await user.dblClick(within(target).getByText('Session 6'));
    const input = screen.getByRole('textbox', { name: 'Session title' });
    expect(input).toHaveFocus();
    await scrollTo(500 * 36 + 4);
    await user.keyboard('{Escape}');
    expect(screen.getByRole('button', { name: 'Select session Session 6' })).toHaveFocus();
  });

  it('skips a disabled rename input in both directions instead of trapping Tab', async () => {
    const { user, fixture } = largeSidebar();
    await user.dblClick(within(screen.getByRole('button', { name: 'Select session Session 6' })).getByText('Session 6'));
    const pending = deferred<Awaited<ReturnType<ManagerClient['api']['renameSession']>>>();
    fixture.api.renameSession.mockReturnValueOnce(pending.promise);
    await user.keyboard('{Enter}');
    expect(screen.getByRole('textbox', { name: 'Session title' })).toBeDisabled();
    act(() => screen.getByRole('button', { name: 'Select session Session 5' }).focus());
    await user.tab();
    expect(screen.getByRole('button', { name: 'Select session Session 7' })).toHaveFocus();
    await user.tab({ shift: true });
    expect(screen.getByRole('button', { name: 'Select session Session 5' })).toHaveFocus();
    await act(async () => { pending.resolve(success(session(6))); await flush(); });
    expect(screen.getByRole('button', { name: 'Select session Session 5' })).toHaveFocus();
  });

  it('delays selection reveal while hidden, then reveals it after resize without snapping back later', async () => {
    const layout = sidebarLayout();
    act(() => layout.resize(0));
    const items = Array.from({ length: 1000 }, (_, index) => session(index + 1));
    const client = createManagerClient(mockApi().api); clients.push(client);
    const { container } = render(<SessionSidebar client={client} sessions={items} selectedId={items[799]!.id} busy={{}} invalidation={0} pageSize={120} />);
    const scroll = container.querySelector<HTMLDivElement>('.sidebar-scroll')!;
    expect(scroll.scrollTop).toBe(0);
    act(() => layout.resize(180));
    await waitFor(() => expect(scroll.scrollTop).toBeGreaterThan(700 * 36));
    expect(screen.getByRole('button', { name: 'Select session Session 800' })).toHaveAttribute('aria-pressed', 'true');
    scroll.scrollTop = 4;
    fireEvent.scroll(scroll);
    await screen.findByRole('button', { name: 'Select session Session 2' });
    act(() => layout.resize(720));
    await screen.findByRole('button', { name: 'Select session Session 20' });
    expect(scroll.scrollTop).toBe(4);
  });

  it('remeasures the window when the scroll viewport resizes', async () => {
    const layout = sidebarLayout();
    const items = Array.from({ length: 1000 }, (_, index) => session(index + 1));
    const client = createManagerClient(mockApi().api); clients.push(client);
    const { container } = render(<SessionSidebar client={client} sessions={items} selectedId={null} busy={{}} invalidation={0} pageSize={120} />);
    const before = container.querySelectorAll('.session-row').length;
    act(() => layout.resize(720));
    await waitFor(() => expect(container.querySelectorAll('.session-row').length).toBeGreaterThan(before));
    expect(container.querySelectorAll('.session-row').length).toBeLessThan(40);
  });

  it('reveals selection in a small archive below a large live list', async () => {
    const { fixture, user, props, rerender, scroll } = largeSidebar(200);
    const items = Array.from({ length: 10 }, (_, index) => archived(10000 + index, `Small archive ${index + 1}`));
    fixture.api.getHistory.mockImplementation(async query => success({ items, total: items.length, page: query.page, pageSize: query.pageSize }));
    await user.click(await screen.findByRole('button', { name: /Archived ·/ }));
    await screen.findByRole('button', { name: 'Select session Small archive 10' });
    rerender(<SessionSidebar {...props} selectedId={items[9]!.id} />);
    expect(scroll.scrollTop).toBeGreaterThan(200 * 36);
    expect(screen.getByRole('button', { name: 'Select session Small archive 10' })).toHaveAttribute('aria-pressed', 'true');
  });

  it('windows archive pages in the same scrollbar and keeps More and keyboard entry reachable', async () => {
    const { fixture, user, scroll, container, props, rerender } = largeSidebar(200);
    const items = Array.from({ length: 240 }, (_, index) => archived(10000 + index, `Archive ${index + 1}`));
    fixture.api.getHistory.mockImplementation(async query => success({ items: items.slice((query.page - 1) * query.pageSize, query.page * query.pageSize), total: items.length, page: query.page, pageSize: query.pageSize }));
    await user.click(await screen.findByRole('button', { name: /Archived ·/ }));
    await screen.findByRole('button', { name: 'Select session Archive 1' });
    await user.tab();
    expect(screen.getByRole('button', { name: 'Select session Archive 1' })).toHaveFocus();
    expect(scroll.scrollTop).toBeGreaterThan(200 * 36 - 180);
    await user.keyboard('{End}');
    expect(screen.getByRole('button', { name: 'Select session Archive 120' })).toHaveFocus();
    await user.tab();
    expect(screen.getByRole('button', { name: 'More' })).toHaveFocus();
    await user.tab({ shift: true });
    expect(screen.getByRole('button', { name: 'Select session Archive 120' })).toHaveFocus();
    await user.click(screen.getByRole('button', { name: 'More' }));
    await screen.findByRole('button', { name: 'Select session Archive 240' });
    expect(screen.getByRole('button', { name: 'Select session Archive 240' })).toHaveFocus();
    expect(screen.queryByRole('button', { name: 'More' })).not.toBeInTheDocument();
    expect(container.querySelectorAll('.session-row').length).toBeLessThan(40);
    await user.click(screen.getByRole('button', { name: 'Select session Archive 240' }));
    expect(fixture.api.activateSession).not.toHaveBeenCalled();
    // Removing live rows moves the archive upward; remeasure its new offset.
    scroll.scrollTop = 36 + 32 + 4 + 230 * 36;
    rerender(<SessionSidebar {...props} sessions={props.sessions.slice(0, 1)} />);
    await screen.findByRole('button', { name: 'Select session Archive 231' });
    await user.click(screen.getByRole('button', { name: 'Archived · 240' }));
    expect(screen.queryByRole('region', { name: 'Archived sessions' })).not.toBeInTheDocument();
  });
});

describe('incremental archive loading', () => {
  async function setup() {
    const view = largeSidebar(200);
    const items = Array.from({ length: 400 }, (_, index) => archived(10000 + index, `Archive ${index + 1}`));
    view.fixture.api.getHistory.mockImplementation(async query => success({ items: items.slice((query.page - 1) * query.pageSize, query.page * query.pageSize), total: items.length, page: query.page, pageSize: query.pageSize }));
    await view.user.click(await screen.findByRole('button', { name: /Archived ·/ }));
    await screen.findByRole('button', { name: 'Select session Archive 120' });
    const requests = () => view.fixture.api.getHistory.mock.calls.filter(([query]) => query.pageSize === 120).map(([query]) => query.page);
    return { ...view, items, requests };
  }

  it('requests only the next page on More, but refreshes the loaded range after invalidation', async () => {
    const { user, requests, props, rerender } = await setup();
    expect(requests()).toEqual([1]);
    await user.click(screen.getByRole('button', { name: 'More' }));
    await screen.findByRole('button', { name: 'Select session Archive 240' });
    expect(requests()).toEqual([1, 2]);
    await user.click(screen.getByRole('button', { name: 'More' }));
    await screen.findByRole('button', { name: 'Select session Archive 360' });
    expect(requests()).toEqual([1, 2, 3]);
    rerender(<SessionSidebar {...props} archiveInvalidation={1} />);
    await waitFor(() => expect(screen.getByRole('button', { name: 'More' })).toBeEnabled());
    expect(requests()).toEqual([1, 2, 3, 1, 2, 3]);
    await user.click(screen.getByRole('button', { name: 'More' }));
    await screen.findByRole('button', { name: 'Select session Archive 400' });
    expect(requests()).toEqual([1, 2, 3, 1, 2, 3, 4]);
  });

  it('reveals an external archive selection when its page arrives, without loading pages merely on scroll', async () => {
    const { user, props, rerender, items, scroll, requests } = await setup();
    rerender(<SessionSidebar {...props} selectedId={items[299]!.id} />);
    scroll.scrollTop = 200 * 36 + 32 + 4 + 100 * 36;
    fireEvent.scroll(scroll);
    await screen.findByRole('button', { name: 'Select session Archive 101' });
    expect(requests()).toEqual([1]);
    await user.click(screen.getByRole('button', { name: 'More' }));
    await screen.findByRole('button', { name: 'Select session Archive 240' });
    await user.click(screen.getByRole('button', { name: 'More' }));
    await screen.findByRole('button', { name: 'Select session Archive 300' });
    expect(screen.getByRole('button', { name: 'Select session Archive 300' })).toHaveAttribute('aria-pressed', 'true');
    expect(scroll.scrollTop).toBeGreaterThan(200 * 36 + 32 + 290 * 36);
    expect(requests()).toEqual([1, 2, 3]);
  });

  it('repairs shifted page offsets if the total changes before an invalidation arrives', async () => {
    const { user, items, requests } = await setup();
    items.unshift(archived(99000, 'New archive'));
    await user.click(screen.getByRole('button', { name: 'More' }));
    await screen.findByRole('button', { name: 'Select session New archive' });
    expect(screen.getByRole('button', { name: 'Select session Archive 239' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Archived · 401' })).toBeInTheDocument();
    expect(requests()).toEqual([1, 2, 1]);
    await user.click(screen.getByRole('button', { name: 'More' }));
    await screen.findByRole('button', { name: 'Select session Archive 359' });
    expect(requests()).toEqual([1, 2, 1, 3]);
  });

  it('returns More focus to the toggle if the archive becomes empty during loading', async () => {
    const { user, fixture } = await setup();
    fixture.api.getHistory.mockImplementation(async query => success({ items: [], total: 0, page: query.page, pageSize: query.pageSize }));
    await user.click(screen.getByRole('button', { name: 'More' }));
    const toggle = await screen.findByRole('button', { name: 'Archived · 0' });
    expect(screen.queryByRole('button', { name: 'More' })).not.toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Select session Archive 120' })).not.toBeInTheDocument();
    expect(toggle).toHaveFocus();
  });

  it('retains loaded items on failure and retries the same next page', async () => {
    const { user, requests, fixture } = await setup();
    fixture.api.getHistory.mockResolvedValueOnce(failure('INTERNAL', 'Try again'));
    await user.click(screen.getByRole('button', { name: 'More' }));
    await waitFor(() => expect(screen.getByRole('button', { name: 'More' })).toBeEnabled());
    expect(screen.getByRole('button', { name: 'Select session Archive 120' })).toBeInTheDocument();
    expect(requests()).toEqual([1, 2]);
    await user.click(screen.getByRole('button', { name: 'More' }));
    await screen.findByRole('button', { name: 'Select session Archive 240' });
    expect(requests()).toEqual([1, 2, 2]);
  });

  it('ignores a stale incremental response after invalidation and page-size changes', async () => {
    const { user, requests, fixture, props, rerender } = await setup();
    const pending = deferred<Awaited<ReturnType<ManagerClient['api']['getHistory']>>>();
    fixture.api.getHistory.mockReturnValueOnce(pending.promise);
    await user.click(screen.getByRole('button', { name: 'More' }));
    expect(screen.getByRole('button', { name: 'More' })).toBeDisabled();
    rerender(<SessionSidebar {...props} archiveInvalidation={1} />);
    await screen.findByRole('button', { name: 'Select session Archive 240' });
    await act(async () => { pending.resolve(success({ items: [archived(99000, 'Stale page')], total: 400, page: 2, pageSize: 120 })); await flush(); });
    expect(screen.queryByRole('button', { name: 'Select session Stale page' })).not.toBeInTheDocument();
    expect(requests()).toEqual([1, 2, 1, 2]);
    rerender(<SessionSidebar {...props} archiveInvalidation={1} pageSize={60} />);
    await waitFor(() => expect(screen.getByRole('button', { name: 'More' })).toBeEnabled());
    expect(fixture.api.getHistory.mock.calls.slice(-2).map(([query]) => [query.page, query.pageSize])).toEqual([[1, 60], [2, 60]]);
    await user.click(screen.getByRole('button', { name: 'More' }));
    await screen.findByRole('button', { name: 'Select session Archive 180' });
    expect(fixture.api.getHistory.mock.calls.at(-1)![0]).toMatchObject({ page: 3, pageSize: 60 });
  });
});
