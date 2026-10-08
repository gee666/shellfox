// @vitest-environment jsdom
import '@testing-library/jest-dom/vitest';
import './terminal-test-mocks';
import { afterEach, expect, it } from 'vitest';
import { act, cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { App } from './App';
import { failure } from '../shared/contracts';
import { deferred, mockApi, session, snapshot } from './test-fixtures';

afterEach(cleanup);
function fixture() {
  const item = session();
  item.tabs.push({ ...session(2).tabs[0]!, sessionId: item.id, title: 'Hidden', lifecycle: 'closed' },
    { ...session(3).tabs[0]!, sessionId: item.id, title: 'Second' }, { ...session(4).tabs[0]!, sessionId: item.id, title: 'Third' });
  return { item, ...mockApi(snapshot([item, session(5)])) };
}
async function ready(f = fixture()) {
  const user = userEvent.setup(); render(<App api={f.api} />);
  await screen.findByRole('tab', { name: /Shell 1/ }); return { ...f, user };
}
const tab = (name: string) => screen.getByRole('tab', { name: new RegExp(name) });
function drag(source: HTMLElement, target: HTMLElement) {
  const transfer = { effectAllowed: '', dropEffect: '', setData: () => {} };
  fireEvent.dragStart(source, { dataTransfer: transfer });
  fireEvent.dragOver(target, { dataTransfer: transfer });
  fireEvent.drop(target, { dataTransfer: transfer });
  fireEvent.dragEnd(source, { dataTransfer: transfer });
}

it('double-click renames on Enter, trims, retains selection and does not close or launch terminals', async () => {
  const f = await ready();
  await f.user.dblClick(tab('Shell 1'));
  const input = screen.getByRole('textbox', { name: 'Terminal tab name' });
  expect(input).toHaveFocus(); await f.user.clear(input); await f.user.type(input, '  Build 雪  {Enter}');
  await waitFor(() => expect(tab('Build 雪')).toHaveAttribute('aria-selected', 'true'));
  expect(f.api.renameTab).toHaveBeenCalledExactlyOnceWith({ sessionId: f.item.id, tabId: f.item.tabs[0].id, title: 'Build 雪' });
  expect(tab('Build 雪')).toHaveFocus();
  expect(f.api.closeTab).not.toHaveBeenCalled(); expect(f.api.addTab).not.toHaveBeenCalled();
});
it('saves an unchanged displayed title so it becomes a user override', async () => {
  const f = await ready();
  await f.user.dblClick(tab('Shell 1'));
  await f.user.keyboard('{Enter}');
  await waitFor(() => expect(f.api.renameTab).toHaveBeenCalledExactlyOnceWith({ sessionId: f.item.id, tabId: f.item.tabs[0].id, title: 'Shell 1' }));
});
it('supports F2, Escape cancellation, context-menu rename and blur save', async () => {
  const f = await ready();
  tab('Shell 1').focus(); await f.user.keyboard('{F2}');
  let input = screen.getByRole('textbox', { name: 'Terminal tab name' });
  await f.user.clear(input); await f.user.type(input, 'Cancelled{Escape}');
  expect(screen.queryByRole('textbox', { name: 'Terminal tab name' })).not.toBeInTheDocument();
  expect(f.api.renameTab).not.toHaveBeenCalled(); expect(tab('Shell 1')).toHaveFocus();
  fireEvent.contextMenu(tab('Second'));
  await f.user.click(screen.getByRole('menuitem', { name: 'Rename terminal' }));
  input = screen.getByRole('textbox', { name: 'Terminal tab name' }); expect(input).toHaveFocus();
  await f.user.clear(input); await f.user.type(input, 'Renamed second'); await f.user.click(tab('Third'));
  await waitFor(() => expect(tab('Renamed second')).toBeInTheDocument());
  expect(f.api.renameTab).toHaveBeenCalledTimes(1); expect(tab('Third')).toHaveAttribute('aria-selected', 'true');
});
it('keeps the draft on validation or storage failure and can retry', async () => {
  const f = await ready(); await f.user.dblClick(tab('Shell 1'));
  const input = screen.getByRole('textbox', { name: 'Terminal tab name' });
  await f.user.clear(input); await f.user.type(input, ' {Enter}');
  expect(await screen.findByRole('alert')).toHaveTextContent('nonblank'); expect(f.api.renameTab).not.toHaveBeenCalled();
  f.api.renameTab.mockResolvedValueOnce(failure('STORAGE_FAILED', 'Disk full'));
  await f.user.clear(input); await f.user.type(input, 'Retry{Enter}');
  await screen.findByText('Disk full'); expect(input).toHaveValue('Retry');
  await waitFor(() => expect(input).toBeEnabled()); await f.user.click(input); await f.user.keyboard('{Enter}');
  await waitFor(() => expect(tab('Retry')).toBeInTheDocument());
});
it('drag/drop reorders visible tabs, preserves hidden slots and the selected terminal', async () => {
  const f = await ready(); await f.user.click(tab('Second'));
  drag(tab('Third'), tab('Shell 1').parentElement!);
  await waitFor(() => expect(screen.getAllByRole('tab').map(node => node.textContent)).toEqual(['Third', 'Shell 1', 'Second']));
  expect(f.api.reorderTabs).toHaveBeenCalledExactlyOnceWith({ sessionId: f.item.id, tabIds: [f.item.tabs[3].id, f.item.tabs[1].id, f.item.tabs[0].id, f.item.tabs[2].id] });
  expect(tab('Second')).toHaveAttribute('aria-selected', 'true'); expect(f.api.closeTab).not.toHaveBeenCalled();
  drag(tab('Third'), tab('Third').parentElement!); expect(f.api.reorderTabs).toHaveBeenCalledTimes(1);
});
it('ignores external drops and cancels a drag when switching sessions', async () => {
  const f = await ready(), transfer = { setData: () => {}, effectAllowed: '', dropEffect: '' };
  fireEvent.drop(tab('Second').parentElement!, { dataTransfer: transfer });
  expect(f.api.reorderTabs).not.toHaveBeenCalled();
  fireEvent.dragStart(tab('Second'), { dataTransfer: transfer });
  await f.user.click(screen.getByRole('button', { name: 'Select session Session 5' }));
  fireEvent.drop(tab('Shell 1').parentElement!, { dataTransfer: transfer });
  expect(f.api.reorderTabs).not.toHaveBeenCalled();
});
it('supports keyboard and menu moves, keeps navigation keys, and leaves failed order unchanged', async () => {
  const f = await ready(); tab('Shell 1').focus(); await f.user.keyboard('{ArrowRight}');
  expect(tab('Second')).toHaveAttribute('aria-selected', 'true');
  await f.user.keyboard('{Alt>}{ArrowLeft}{/Alt}');
  await waitFor(() => expect(screen.getAllByRole('tab')[0]).toHaveTextContent('Second'));
  expect(tab('Second')).toHaveAttribute('aria-selected', 'true');
  f.api.reorderTabs.mockResolvedValueOnce(failure('STORAGE_FAILED', 'Order not saved'));
  fireEvent.contextMenu(tab('Second')); await f.user.click(screen.getByRole('menuitem', { name: 'Move right' }));
  expect(await screen.findByRole('alert')).toHaveTextContent('Order not saved');
  expect(screen.getAllByRole('tab')[0]).toHaveTextContent('Second');
});
it('does not let delayed metadata responses steal a different session selection', async () => {
  const f = fixture(), rename = f.api.renameTab.getMockImplementation()!, pending = deferred<Awaited<ReturnType<typeof f.api.renameTab>>>();
  f.api.renameTab.mockReturnValueOnce(pending.promise);
  const { user } = await ready(f); await user.dblClick(tab('Shell 1'));
  const input = screen.getByRole('textbox', { name: 'Terminal tab name' });
  await user.clear(input); await user.type(input, 'Delayed{Enter}');
  await user.click(screen.getByRole('button', { name: 'Select session Session 5' }));
  await act(async () => pending.resolve(await rename({ sessionId: f.item.id, tabId: f.item.tabs[0].id, title: 'Delayed' })));
  expect(screen.getByRole('button', { name: 'Select session Session 5' })).toHaveAttribute('aria-pressed', 'true');
  await user.click(screen.getByRole('button', { name: 'Select session Session 1' })); expect(await screen.findByRole('tab', { name: /Delayed/ })).toBeInTheDocument();
});
