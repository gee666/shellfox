// @vitest-environment jsdom
import '@testing-library/jest-dom/vitest';
import { useStore } from 'zustand';
import { afterEach, describe, expect, it } from 'vitest';
import { act, cleanup, render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { failure, success } from '../shared/contracts';
import { SessionSidebar } from './SessionSidebar';
import { createManagerClient } from './store';
import type { ManagerClient } from './store';
import { mockApi, session, snapshot } from './test-fixtures';

const clients: ManagerClient[] = [];
afterEach(() => { cleanup(); clients.splice(0).forEach(client => client.stop()); });
const archived = (id: number, title: string) => session(id, { title, status: 'settled', settledAt: '2026-10-04T11:00:00.000Z' });
function Navigation({ client }: { client: ManagerClient }) {
  const state = useStore(client.store);
  return <SessionSidebar client={client} sessions={state.snapshot?.sessions ?? []} selectedId={state.selectedId} busy={state.busyTabIds} invalidation={state.historyStatusVersion} archiveInvalidation={state.archiveVersion} pageSize={1} />;
}
const flush = async () => { for (let i = 0; i < 8; i++) await Promise.resolve(); };

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
