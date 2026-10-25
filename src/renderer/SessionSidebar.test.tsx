// @vitest-environment jsdom
import '@testing-library/jest-dom/vitest';
import { useStore } from 'zustand';
import { afterEach, describe, expect, it } from 'vitest';
import { act, cleanup, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { failure, success } from '../shared/contracts';
import { SessionSidebar, shortenHome } from './SessionSidebar';
import { createManagerClient } from './store';
import type { ManagerClient } from './store';
import { deferred, mockApi, session, snapshot } from './test-fixtures';

const clients: ManagerClient[] = [];
afterEach(() => { cleanup(); clients.splice(0).forEach(client => client.stop()); });
function Navigation({ client }: { client: ManagerClient }) {
  const state = useStore(client.store);
  return <SessionSidebar client={client} sessions={state.snapshot?.sessions ?? []} selectedId={state.selectedId} busy={state.busyTabIds} pageSize={state.snapshot?.settings.historyPageSize ?? 10} />;
}
const settled = (id: number) => session(id, { status: 'settled', settledAt: '2026-10-04T11:00:00.000Z' });
async function setup(items = [session()], pageSize = 10) {
  const initial = snapshot(items); initial.settings.historyPageSize = pageSize;
  const fixture = mockApi(initial);
  const client = createManagerClient(fixture.api); clients.push(client); client.start();
  const user = userEvent.setup(); const view = render(<Navigation client={client} />);
  await screen.findByRole('button', { name: `Local · ${items.length}` });
  return { ...view, fixture, client, user, initial };
}
const select = (id: number) => screen.getByRole('button', { name: `Select session Session ${id}` });
async function context(user: ReturnType<typeof userEvent.setup>, id: number) {
  await user.pointer({ keys: '[MouseRight]', target: select(id) });
}

describe('sidebar cwd display', () => {
  it.each([
    [String.raw`\\wsl.localhost\Debian\var\www`, '/var/www'],
    [String.raw`\\wsl$\Debian\var\www`, '/var/www'],
    [String.raw`\\wsl$\Debian`, '/'],
    [String.raw`\\wsl$\Debian\home\user\work`, '~/work'],
    [String.raw`C:\Users\user\work`, String.raw`~\work`],
    [String.raw`C:\work`, String.raw`C:\work`],
    [String.raw`\\server\share\work`, String.raw`\\server\share\work`],
    ['/var/www', '/var/www'], ['/home/user/work', '~/work'], ['/Users/user/work', '~/work'],
  ])('formats %s as %s', (cwd, expected) => expect(shortenHome(cwd)).toBe(expected));

  it('keeps the original tooltip and copied path', async () => {
    const cwd = String.raw`\\wsl.localhost\Debian\var\www`;
    const { fixture, user } = await setup([session(1, { cwd })]);
    expect(screen.getByText('/var/www')).toHaveAttribute('title', cwd);
    await context(user, 1); await user.click(screen.getByRole('menuitem', { name: 'Copy path' }));
    expect(fixture.api.copyText).toHaveBeenCalledWith({ text: cwd });
  });
});

describe('local sessions pagination', () => {
  it('includes settled sessions in Local, open by default, and appends one page at a time', async () => {
    const items = Array.from({ length: 23 }, (_, index) => index > 17 ? settled(index + 1) : session(index + 1));
    const { user } = await setup(items);
    const toggle = screen.getByRole('button', { name: 'Local · 23' });
    expect(toggle).toHaveAttribute('aria-expanded', 'true');
    expect(screen.getByRole('region', { name: 'Local sessions' })).toBeVisible();
    expect(screen.queryByText(/Archived/)).not.toBeInTheDocument();
    expect(screen.getAllByRole('listitem')).toHaveLength(10);
    await user.click(screen.getByRole('button', { name: 'Show more' }));
    expect(screen.getAllByRole('listitem')).toHaveLength(20);
    expect(select(19).closest('.session-row')).toHaveClass('settled');
    await user.click(screen.getByRole('button', { name: 'Show more' }));
    expect(screen.getAllByRole('listitem')).toHaveLength(23);
    expect(screen.queryByRole('button', { name: 'Show more' })).not.toBeInTheDocument();
    expect(select(23)).toHaveFocus();
    await user.click(toggle);
    expect(screen.queryByRole('region', { name: 'Local sessions' })).not.toBeInTheDocument();
    await user.click(toggle);
    expect(screen.getAllByRole('listitem')).toHaveLength(23);
  });

  it('keeps loaded pages on snapshot refresh and resets to the new configured page size', async () => {
    const items = Array.from({ length: 8 }, (_, index) => session(index + 1));
    const { user, fixture, initial } = await setup(items, 2);
    await user.click(screen.getByRole('button', { name: 'Show more' }));
    expect(screen.getAllByRole('listitem')).toHaveLength(4);
    act(() => fixture.emit({ ...initial, revision: 2 }, 'native'));
    await waitFor(() => expect(fixture.api.getSnapshot).toHaveBeenCalledTimes(2));
    expect(screen.getAllByRole('listitem')).toHaveLength(4);
    act(() => fixture.emit({ ...initial, revision: 3, settings: { ...initial.settings, historyPageSize: 3 } }, 'settings'));
    await waitFor(() => expect(screen.getAllByRole('listitem')).toHaveLength(3));
    await user.click(screen.getByRole('button', { name: 'Show more' }));
    expect(screen.getAllByRole('listitem')).toHaveLength(6);
  });

  it('shows the empty Local section without extra requests', async () => {
    const { fixture } = await setup([]);
    expect(screen.getByText('No sessions, press +')).toBeVisible();
    expect(screen.queryByRole('button', { name: 'Show more' })).not.toBeInTheDocument();
    expect(fixture.api.getSnapshot).toHaveBeenCalledTimes(1);
  });
});

describe('session actions', () => {
  it('settles and unsets from the right icon without confirmation or terminal changes', async () => {
    const { user, fixture } = await setup();
    await user.click(screen.getByRole('button', { name: 'Settle session Session 1' }));
    expect(fixture.api.settleSession).toHaveBeenCalledWith({ sessionId: session().id, confirmActive: false });
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
    expect(await screen.findByRole('button', { name: 'Unsettle session Session 1' })).toBeVisible();
    expect(within(select(1).closest('.session-row')!).getByLabelText('Settled')).toHaveClass('dot-settled');
    expect(fixture.api.closeTab).not.toHaveBeenCalled();
    await user.click(screen.getByRole('button', { name: 'Unsettle session Session 1' }));
    expect(fixture.api.unsettleSession).toHaveBeenCalledWith({ sessionId: session().id });
    expect(await screen.findByRole('button', { name: 'Settle session Session 1' })).toBeVisible();
  });

  it('offers Settle and Unsettle in the context menu', async () => {
    const { user, fixture } = await setup([session(1), settled(2)]);
    await context(user, 1); await user.click(screen.getByRole('menuitem', { name: 'Settle' }));
    expect(fixture.api.settleSession).toHaveBeenCalledTimes(1);
    await context(user, 2); await user.click(screen.getByRole('menuitem', { name: 'Unsettle' }));
    expect(fixture.api.unsettleSession).toHaveBeenCalledWith({ sessionId: session(2).id });
  });

  it.each([false, true])('confirms deletion for every session, settled=%s', async isSettled => {
    const { user, fixture } = await setup([isSettled ? settled(1) : session(1)]);
    await context(user, 1); await user.click(screen.getByRole('menuitem', { name: 'Delete…' }));
    const dialog = screen.getByRole('dialog', { name: 'Delete session' });
    expect(dialog).toHaveTextContent('Its running terminals will be closed. This cannot be undone.');
    expect(fixture.api.deleteSession).not.toHaveBeenCalled();
    await user.click(screen.getByRole('button', { name: 'Cancel' }));
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
    await context(user, 1); await user.click(screen.getByRole('menuitem', { name: 'Delete…' }));
    await user.click(screen.getByRole('button', { name: 'Delete' }));
    expect(fixture.api.deleteSession).toHaveBeenCalledExactlyOnceWith({ sessionId: session(1).id });
    expect(await screen.findByRole('button', { name: 'Local · 0' })).toBeVisible();
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
  });

  it('keeps a failed deletion open and does not mention closing terminals if none are live', async () => {
    const item = settled(1); item.tabs[0]!.lifecycle = 'closed';
    const { user, fixture } = await setup([item]);
    fixture.api.deleteSession.mockResolvedValueOnce(failure('STORAGE_FAILED', 'Try again.'));
    await context(user, 1); await user.click(screen.getByRole('menuitem', { name: 'Delete…' }));
    expect(screen.getByRole('dialog')).not.toHaveTextContent('terminals');
    await user.click(screen.getByRole('button', { name: 'Delete' }));
    expect(screen.getByRole('dialog')).toBeVisible(); expect(select(1)).toBeVisible();
  });

  it('opens a terminal when selecting a settled session with no open tabs', async () => {
    const item = settled(1); item.tabs[0]!.lifecycle = 'closed';
    const { fixture, client, user } = await setup([item]);
    const reopened = settled(1);
    fixture.api.activateSession.mockImplementationOnce(async () => { fixture.emit({ ...snapshot([reopened]), revision: 100 }); return success(reopened); });
    await user.click(select(1));
    expect(fixture.api.activateSession).toHaveBeenCalledWith({ sessionId: item.id });
    await waitFor(() => expect(client.store.getState().activeTabIds[item.id]).toBe(reopened.tabs[0]!.id));
  });

  it.each([false, true])('touches embedded selection without resetting its active tab, settled=%s', async isSettled => {
    const item = isSettled ? settled(1) : session(1);
    const second = { ...session(20).tabs[0]!, sessionId: item.id };
    item.tabs.push(second);
    const { user, fixture, client } = await setup([item]);
    act(() => client.selectTab(item, second.id));
    const pending = deferred<Awaited<ReturnType<ManagerClient['api']['activateSession']>>>();
    fixture.api.activateSession.mockReturnValueOnce(pending.promise);
    await user.click(select(1));
    expect(fixture.api.activateSession).toHaveBeenCalledExactlyOnceWith({ sessionId: item.id });
    expect(fixture.api.addTab).not.toHaveBeenCalled();
    await act(async () => { pending.resolve(success(item)); });
    expect(client.store.getState().activeTabIds[item.id]).toBe(second.id);
    expect(fixture.api.closeTab).not.toHaveBeenCalled();
  });

  it('ignores an activation result after selecting a different session', async () => {
    const { user, fixture, client } = await setup([session(1), session(2)]);
    const pending = deferred<Awaited<ReturnType<ManagerClient['api']['activateSession']>>>();
    fixture.api.activateSession.mockReturnValueOnce(pending.promise);
    await user.click(select(1)); await user.click(select(2));
    await act(async () => { pending.resolve(success(session(1))); });
    expect(client.store.getState().selectedId).toBe(session(2).id);
  });

  it('does not activate legacy external sessions that already have a terminal', async () => {
    const item = session(1, { adapterId: 'windows-terminal' }); item.tabs[0]!.terminalKind = 'external-legacy';
    const { user, fixture } = await setup([item]);
    await user.click(select(1));
    expect(fixture.api.activateSession).not.toHaveBeenCalled();
  });

  it('pins settled sessions too, keeps their marker, and can unpin them', async () => {
    const { user, fixture } = await setup([session(1), settled(2)]);
    await context(user, 2); await user.click(screen.getByRole('menuitem', { name: 'Pin to top' }));
    expect(fixture.api.setSessionPinned).toHaveBeenCalledWith({ sessionId: session(2).id, pinned: true });
    expect(await screen.findByLabelText('Pinned')).toBeVisible();
    expect(screen.getAllByRole('listitem')[0]).toHaveAttribute('data-session-id', session(2).id);
    await context(user, 2); await user.click(screen.getByRole('menuitem', { name: 'Unpin' }));
    await waitFor(() => expect(screen.queryByLabelText('Pinned')).not.toBeInTheDocument());
  });
});
