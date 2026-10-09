// @vitest-environment jsdom
import '@testing-library/jest-dom/vitest';
import './terminal-test-mocks';
import { useState } from 'react';
import { act, cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import type { SessionDto } from '../shared/contracts';
import { failure } from '../shared/contracts';
import { TerminalTabs } from './TerminalTabs';
import { createManagerClient } from './store';
import { deferred, mockApi, session, snapshot } from './test-fixtures';
import { TAB_WINDOW_WIDTH } from './terminal-tab-window';

const renders = vi.hoisted(() => vi.fn());
vi.mock('./components', async importOriginal => {
  const actual = await importOriginal<typeof import('./components')>();
  return { ...actual, StatusDot: (props: Parameters<typeof actual.StatusDot>[0]) => { renders(props.state); return actual.StatusDot(props); } };
});
beforeEach(() => renders.mockClear());
afterEach(() => { cleanup(); document.body.replaceChildren(); vi.restoreAllMocks(); });

function setup(count = 1000, hidden = false) {
  const initial = session();
  initial.tabs = Array.from({ length: count }, (_, i) => ({ ...session(i + 1).tabs[0], sessionId: initial.id, title: `Tab ${i + 1}`, ordinal: i, status: 'running' as const }));
  if (hidden) { initial.tabs[4].lifecycle = 'closed'; initial.tabs[7].terminalKind = 'external-legacy'; }
  const f = mockApi(snapshot([initial])); const client = createManagerClient(f.api);
  client.store.setState({ selectedId: initial.id });
  const selected = vi.fn(), closed = vi.fn();
  let controls!: { session: (value: SessionDto) => void; selected: (id: string) => void; busy: (value: Record<string, boolean>) => void; disabled: (value: boolean) => void };
  vi.spyOn(client, 'accept').mockImplementation(value => controls.session(value));
  vi.spyOn(client, 'refresh').mockImplementation(() => {});
  function Harness() {
    const [item, setItem] = useState<SessionDto>(initial);
    const [active, setActive] = useState(initial.tabs[0].id);
    const [busy, setBusy] = useState<Record<string, boolean>>({});
    const [disabled, setDisabled] = useState(false);
    controls = { session: setItem, selected: setActive, busy: setBusy, disabled: setDisabled };
    return <TerminalTabs session={item} tabs={item.tabs.filter(tab => tab.lifecycle !== 'closed' && tab.terminalKind !== 'external-legacy')} selectedId={active} client={client} busy={busy} disabled={disabled} editable
      canClose={() => true} onSelect={tab => { selected(tab); setActive(tab.id); }} onClose={closed} />;
  }
  const view = render(<Harness />);
  const root = screen.getByRole('tablist');
  async function scroll(left: number) {
    act(() => { root.scrollLeft = left; fireEvent.scroll(root); });
    await act(async () => { await new Promise(resolve => setTimeout(resolve, 20)); });
  }
  function update(item: SessionDto) { f.emit(snapshot([item])); act(() => controls.session(item)); }
  return { ...f, initial, client, root, view, selected, closed, scroll, update, controls: () => controls };
}
const tab = (number: number) => screen.getByRole('tab', { name: new RegExp(`Tab ${number}$`) });
const transfer = () => ({ effectAllowed: '', dropEffect: '', setData: vi.fn() });
const dragOverAt = (target: HTMLElement, dataTransfer: ReturnType<typeof transfer>, clientX: number) => fireEvent(target, Object.assign(new MouseEvent('dragover', { bubbles: true, cancelable: true, clientX }), { dataTransfer }));

it('bounds a 1000-tab strip and reveals external selection without stealing focus', async () => {
  const f = setup(); expect(screen.getAllByRole('tab').length).toBeLessThan(20);
  expect(f.root).toHaveClass('terminal-tabs-windowed');
  const outside = document.createElement('button'); document.body.append(outside); outside.focus();
  act(() => f.controls().selected(f.initial.tabs[999].id));
  expect(tab(1000)).toHaveAttribute('aria-selected', 'true'); expect(tab(1000)).toHaveAttribute('aria-posinset', '1000'); expect(tab(1000)).toHaveAttribute('aria-setsize', '1000');
  expect(f.root.scrollLeft).toBe(1000 * TAB_WINDOW_WIDTH - 800); expect(outside).toHaveFocus(); outside.remove();
  await f.scroll(400 * TAB_WINDOW_WIDTH);
  expect(tab(1000)).toBeInTheDocument(); expect(screen.getAllByRole('tab').length).toBeLessThan(20);
  // Fresh activity snapshots must not undo the user's manual scroll.
  f.update({ ...f.initial, tabs: f.initial.tabs.map(tab => ({ ...tab })) });
  expect(f.root.scrollLeft).toBe(400 * TAB_WINDOW_WIDTH);
  expect(f.api.closeTab).not.toHaveBeenCalled(); expect(f.api.addTab).not.toHaveBeenCalled();
});

it('skips unchanged rows, including fresh DTOs, and invokes current action data', () => {
  const f = setup(); renders.mockClear();
  act(() => f.controls().busy({ [f.initial.tabs[999].id]: true })); expect(renders).not.toHaveBeenCalled();
  act(() => f.controls().busy({ [f.initial.tabs[2].id]: true })); expect(renders).toHaveBeenCalledOnce();
  renders.mockClear();
  const changed = { ...f.initial, tabs: f.initial.tabs.map(tab => ({ ...tab, agents: 9 })) };
  f.update(changed); expect(renders).not.toHaveBeenCalled();
  fireEvent.click(tab(3)); expect(f.selected).toHaveBeenLastCalledWith(changed.tabs[2]);
  fireEvent.click(screen.getByRole('button', { name: 'Close terminal Tab 3' })); expect(f.closed).toHaveBeenLastCalledWith(changed.tabs[2]);
});

it('preserves normal strip sizing and scrolls variable-width selection into view', () => {
  const f = setup(10); expect(screen.getAllByRole('tab')).toHaveLength(10); expect(f.root).not.toHaveClass('terminal-tabs-windowed');
  vi.spyOn(f.root, 'getBoundingClientRect').mockReturnValue({ left: 100 } as DOMRect);
  const row = tab(10).parentElement!;
  vi.spyOn(row, 'getBoundingClientRect').mockReturnValue({ left: 1000, width: 150 } as DOMRect);
  act(() => f.controls().selected(f.initial.tabs[9].id)); expect(f.root.scrollLeft).toBe(250);
});

it('mounts before keyboard focus for End, Home and wrapping arrows', () => {
  const f = setup(); act(() => tab(1).focus());
  fireEvent.keyDown(tab(1), { key: 'End' });
  expect(tab(1000)).toHaveFocus(); expect(tab(1000)).toHaveAttribute('aria-selected', 'true');
  fireEvent.keyDown(tab(1000), { key: 'ArrowRight' }); expect(tab(1)).toHaveFocus(); expect(f.root.scrollLeft).toBe(0);
  fireEvent.keyDown(tab(1), { key: 'ArrowLeft' }); expect(tab(1000)).toHaveFocus();
  fireEvent.keyDown(tab(1000), { key: 'Home' }); expect(tab(1)).toHaveFocus();
  expect(screen.getAllByRole('tab').length).toBeLessThan(20);
  expect(f.api.reorderTabs).not.toHaveBeenCalled();
});

it('keeps keyboard close reachable through the selected tab and retains focused rows while scrolling', async () => {
  const f = setup(); const user = userEvent.setup(); act(() => tab(1).focus());
  await user.tab(); expect(screen.getByRole('button', { name: 'Close terminal Tab 1' })).toHaveFocus();
  await f.scroll(500 * TAB_WINDOW_WIDTH);
  expect(screen.getByRole('button', { name: 'Close terminal Tab 1' })).toHaveFocus();
  await user.keyboard('{Enter}'); expect(f.closed).toHaveBeenCalledExactlyOnceWith(f.initial.tabs[0]);
});

it('retains a rename draft across scrolling, failure and cancellation, then restores focus', async () => {
  const f = setup(); const user = userEvent.setup();
  act(() => f.controls().selected(f.initial.tabs[499].id));
  fireEvent.contextMenu(tab(502)); await user.click(screen.getByRole('menuitem', { name: 'Rename terminal' }));
  const input = screen.getByRole('textbox', { name: 'Terminal tab name' }); expect(input).toHaveFocus();
  await user.clear(input); await user.type(input, 'Renamed draft'); await f.scroll(0);
  expect(input).toHaveFocus(); expect(input).toHaveValue('Renamed draft'); expect(screen.getAllByRole('tab').length).toBeLessThan(20);
  f.api.renameTab.mockResolvedValueOnce(failure('STORAGE_FAILED', 'Not saved'));
  await user.keyboard('{Enter}'); await waitFor(() => expect(input).not.toBeDisabled());
  expect(input).toHaveValue('Renamed draft'); expect(input).toHaveFocus();
  await user.keyboard('{Escape}'); expect(tab(502)).toHaveFocus(); expect(f.root.scrollLeft).toBeGreaterThan(0);
  expect(tab(500)).toHaveAttribute('aria-selected', 'true'); expect(f.api.renameTab).toHaveBeenCalledTimes(1);
});

it('saves an offscreen rename and keeps its focus target mounted through completion', async () => {
  const f = setup(); const user = userEvent.setup();
  act(() => f.controls().selected(f.initial.tabs[499].id)); act(() => tab(502).focus());
  fireEvent.keyDown(tab(502), { key: 'F2' });
  const input = screen.getByRole('textbox', { name: 'Terminal tab name' }); await user.clear(input); await user.type(input, 'Build long');
  const pending = deferred<Awaited<ReturnType<typeof f.api.renameTab>>>(); const rename = f.api.renameTab.getMockImplementation()!;
  f.api.renameTab.mockReturnValueOnce(pending.promise); await user.keyboard('{Enter}'); await f.scroll(0);
  // Focus may leave a disabled input while a save is pending.
  const outside = document.createElement('button'); document.body.append(outside);
  act(() => { outside.focus(); fireEvent.blur(input, { relatedTarget: outside }); }); expect(outside).toHaveFocus();
  expect(input).toBeInTheDocument();
  await act(async () => pending.resolve(await rename({ sessionId: f.initial.id, tabId: f.initial.tabs[501].id, title: 'Build long' })));
  expect(screen.getByRole('tab', { name: /Build long$/ })).toHaveFocus(); expect(f.root.scrollLeft).toBeGreaterThan(0);
  expect(f.api.renameTab).toHaveBeenCalledTimes(1);
});

it('retains a menu target while scrolled away and preserves hidden slots on menu moves', async () => {
  const f = setup(1000, true); const user = userEvent.setup();
  fireEvent.keyDown(tab(1), { key: 'End' });
  fireEvent.keyDown(tab(1000), { key: 'F10', shiftKey: true }); await f.scroll(0);
  expect(tab(1000)).toBeInTheDocument();
  await user.click(screen.getByRole('menuitem', { name: 'Move left' }));
  await waitFor(() => expect(f.api.reorderTabs).toHaveBeenCalledOnce());
  const order = f.api.reorderTabs.mock.calls[0][0].tabIds;
  expect(order).toHaveLength(1000); expect(order[4]).toBe(f.initial.tabs[4].id); expect(order[7]).toBe(f.initial.tabs[7].id);
  expect(order.slice(-2)).toEqual([f.initial.tabs[999].id, f.initial.tabs[998].id]); expect(tab(1000)).toHaveAttribute('aria-selected', 'true');
});

it('pins a native drag source across window changes and drops into the full saved permutation', async () => {
  const f = setup(1000, true); const dataTransfer = transfer(); const source = tab(1);
  fireEvent.dragStart(source, { dataTransfer }); await f.scroll(700 * TAB_WINDOW_WIDTH);
  expect(source).toBeInTheDocument(); expect(source).toHaveAttribute('draggable', 'true');
  const target = tab(702).parentElement!; fireEvent.dragOver(target, { dataTransfer, clientX: 400 }); fireEvent.drop(target, { dataTransfer, clientX: 400 }); fireEvent.dragEnd(source, { dataTransfer });
  await waitFor(() => expect(f.api.reorderTabs).toHaveBeenCalledOnce());
  const order = f.api.reorderTabs.mock.calls[0][0].tabIds;
  expect(order).toHaveLength(1000); expect(new Set(order).size).toBe(1000);
  expect(order[4]).toBe(f.initial.tabs[4].id); expect(order[7]).toBe(f.initial.tabs[7].id);
  expect(order[701]).toBe(f.initial.tabs[0].id); expect(tab(1)).toHaveAttribute('aria-selected', 'true');
});

it('resolves a drop over a spacer during the frame before a scroll window catches up', async () => {
  const f = setup(); const dataTransfer = transfer(); fireEvent.dragStart(tab(1), { dataTransfer });
  act(() => { f.root.scrollLeft = 500 * TAB_WINDOW_WIDTH; });
  fireEvent(f.root, Object.assign(new MouseEvent('drop', { bubbles: true, cancelable: true, clientX: 300 }), { dataTransfer }));
  await waitFor(() => expect(f.api.reorderTabs).toHaveBeenCalledOnce());
  expect(f.api.reorderTabs.mock.calls[0][0].tabIds[501]).toBe(f.initial.tabs[0].id);
});

it('scrolls during an edge drag and cancels its frame on drag end and unmount', () => {
  const f = setup(); const frames: FrameRequestCallback[] = [];
  const requested = vi.spyOn(globalThis, 'requestAnimationFrame').mockImplementation(callback => { frames.push(callback); return frames.length - 1; });
  const cancelled = vi.spyOn(globalThis, 'cancelAnimationFrame').mockImplementation(() => {});
  vi.spyOn(f.root, 'getBoundingClientRect').mockReturnValue({ left: 0, right: 800 } as DOMRect);
  const dataTransfer = transfer(); fireEvent.dragStart(tab(1), { dataTransfer });
  dragOverAt(tab(7), dataTransfer, 790); expect(requested).toHaveBeenCalledOnce();
  act(() => frames[0](0)); expect(f.root.scrollLeft).toBe(18);
  fireEvent.dragEnd(tab(1), { dataTransfer }); expect(cancelled).toHaveBeenLastCalledWith(1);
  fireEvent.dragStart(tab(1), { dataTransfer }); dragOverAt(tab(7), dataTransfer, 790);
  f.view.unmount(); expect(cancelled).toHaveBeenLastCalledWith(2);
  expect(f.api.reorderTabs).not.toHaveBeenCalled();
});

it('coalesces resize notifications, keeps selection visible after shrink, and cancels pending measurement', () => {
  let resize = () => {}; const disconnect = vi.fn();
  vi.spyOn(globalThis, 'ResizeObserver').mockImplementation(class {
    constructor(callback: ResizeObserverCallback) { resize = () => callback([], this); }
    observe = vi.fn(); unobserve = vi.fn(); disconnect = disconnect;
  });
  const frames: FrameRequestCallback[] = [];
  const requested = vi.spyOn(globalThis, 'requestAnimationFrame').mockImplementation(callback => { frames.push(callback); return frames.length - 1; });
  const cancelled = vi.spyOn(globalThis, 'cancelAnimationFrame').mockImplementation(() => {});
  const f = setup(); let width = 800;
  Object.defineProperty(f.root, 'clientWidth', { configurable: true, get: () => width });
  act(() => f.controls().selected(f.initial.tabs[999].id)); width = 360;
  for (let i = 0; i < 100; i++) resize(); expect(requested).toHaveBeenCalledOnce();
  act(() => frames[0](0)); expect(f.root.scrollLeft).toBe(1000 * TAB_WINDOW_WIDTH - 360);
  expect(tab(1000)).toHaveAttribute('aria-selected', 'true'); expect(screen.getAllByRole('tab').length).toBeLessThan(12);
  resize(); f.view.unmount(); expect(disconnect).toHaveBeenCalledOnce(); expect(cancelled).toHaveBeenLastCalledWith(1);
});

it('ignores external drops, blocks locked reorder, and clamps the window after shrinking', async () => {
  const f = setup(); fireEvent.drop(tab(2), { dataTransfer: transfer() }); expect(f.api.reorderTabs).not.toHaveBeenCalled();
  act(() => f.controls().disabled(true));
  fireEvent.dragStart(tab(1), { dataTransfer: transfer() }); fireEvent.drop(tab(2), { dataTransfer: transfer() });
  expect(f.api.reorderTabs).not.toHaveBeenCalled();
  act(() => f.controls().disabled(false));
  fireEvent.keyDown(tab(1), { key: 'End' });
  f.update({ ...f.initial, tabs: f.initial.tabs.slice(0, 20) });
  act(() => f.controls().selected(f.initial.tabs[19].id));
  expect(screen.getAllByRole('tab')).toHaveLength(20); expect(f.root).not.toHaveClass('terminal-tabs-windowed');
});
