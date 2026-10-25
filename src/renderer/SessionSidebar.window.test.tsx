// @vitest-environment jsdom
import '@testing-library/jest-dom/vitest';
import { afterEach, expect, it, vi } from 'vitest';
import { act, cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { SessionSidebar } from './SessionSidebar';
import { createManagerClient } from './store';
import { mockApi, session, snapshot } from './test-fixtures';

afterEach(() => { cleanup(); vi.restoreAllMocks(); vi.unstubAllGlobals(); });
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
    const top = 28 - scroll.scrollTop;
    return { top, bottom: top + this.children.length * 36, left: 0, right: 220, width: 220, height: 180, x: 0, y: top, toJSON() {} };
  });
  return { resize: (value: number) => { height = value; observers.forEach(callback => callback()); } };
}
function setup(count = 1000) {
  const layout = sidebarLayout();
  const items = Array.from({ length: count }, (_, index) => session(index + 1));
  const fixture = mockApi(snapshot(items)); const client = createManagerClient(fixture.api);
  const props = { client, sessions: items, selectedId: null as string | null, busy: {}, pageSize: 100 };
  const view = render(<SessionSidebar {...props} />);
  // Exercise the loaded range after a user has expanded all pages.
  while (screen.queryByRole('button', { name: 'Show more' })) fireEvent.click(screen.getByRole('button', { name: 'Show more' }));
  const scroll = view.container.querySelector<HTMLDivElement>('.sidebar-scroll')!;
  const scrollTo = async (index: number) => {
    scroll.scrollTop = index * 36 + 28; fireEvent.scroll(scroll);
    await screen.findByRole('button', { name: `Select session Session ${index + 1}` });
  };
  return { ...view, layout, fixture, client, props, items, scroll, scrollTo, user: userEvent.setup() };
}
const row = (id: number) => screen.getByRole('button', { name: `Select session Session ${id}` });

it('bounds mounted rows and preserves loaded height and positions', async () => {
  const { container, scrollTo } = setup(2000);
  const list = screen.getByRole('list');
  expect(container.querySelectorAll('.session-row').length).toBeLessThan(30);
  const fullHeight = Array.from(list.children).reduce((sum, child) => sum + (child.getAttribute('role') === 'listitem' ? 36 : Number.parseFloat((child as HTMLElement).style.height)), 0);
  expect(fullHeight).toBe(2000 * 36);
  await scrollTo(1000);
  expect(row(1001).closest('[role="listitem"]')).toHaveAttribute('aria-posinset', '1001');
  expect(row(1001).closest('[role="listitem"]')).toHaveAttribute('aria-setsize', '2000');
  expect(container.querySelectorAll('.session-row').length).toBeLessThan(30);
});

it('keeps focused and menu rows mounted while scrolling, then releases them', async () => {
  const { scrollTo, user } = setup();
  const target = row(6); act(() => target.focus());
  await user.keyboard('{Shift>}{F10}{/Shift}');
  expect(screen.getByRole('menu')).toBeVisible();
  await scrollTo(500);
  expect(target).toBeInTheDocument();
  await user.keyboard('{Escape}'); expect(target).toHaveFocus();
  await scrollTo(500);
  act(() => row(501).focus());
  await waitFor(() => expect(target).not.toBeInTheDocument());
});

it('reaches row actions with Tab, crosses window boundaries, and supports Home/End', async () => {
  const { user, container } = setup();
  act(() => row(1).focus());
  await user.tab(); expect(screen.getByRole('button', { name: 'Settle session Session 1' })).toHaveFocus();
  await user.tab(); expect(row(2)).toHaveFocus();
  for (let index = 0; index < 30; index++) await user.tab();
  expect(row(17)).toHaveFocus();
  expect(container.querySelectorAll('.session-row').length).toBeLessThan(35);
  await user.keyboard('{End}'); expect(row(1000)).toHaveFocus();
  await user.tab(); expect(screen.getByRole('button', { name: 'Settle session Session 1000' })).toHaveFocus();
  await user.tab(); expect(screen.getByRole('button', { name: 'Local · 1000' })).not.toHaveFocus();
  await user.tab({ shift: true }); expect(screen.getByRole('button', { name: 'Settle session Session 1000' })).toHaveFocus();
  await user.keyboard('{Home}'); expect(row(1)).toHaveFocus();
});

it('retains rename text while scrolling and returns focus after cancellation', async () => {
  const { user, scrollTo } = setup();
  await user.dblClick(within(row(6)).getByText('Session 6'));
  const input = screen.getByRole('textbox', { name: 'Session title' });
  await user.clear(input); await user.type(input, 'Kept name');
  await scrollTo(500);
  expect(input).toHaveValue('Kept name'); expect(input).toHaveFocus();
  await user.keyboard('{Escape}'); expect(row(6)).toHaveFocus();
});

it('remeasures the viewport on resize without pulling focus or snapping back to selection', async () => {
  const { layout, container, scrollTo, props, items, rerender, scroll } = setup();
  const before = container.querySelectorAll('.session-row').length;
  act(() => layout.resize(720));
  await waitFor(() => expect(container.querySelectorAll('.session-row').length).toBeGreaterThan(before));
  rerender(<SessionSidebar {...props} pageSize={1000} selectedId={items[799]!.id} />);
  expect(row(800)).toHaveAttribute('aria-pressed', 'true');
  expect(scroll.scrollTop).toBeGreaterThan(700 * 36);
  await scrollTo(0);
  rerender(<SessionSidebar {...props} pageSize={1000} selectedId={items[799]!.id} busy={{ [items[799]!.tabs[0]!.id]: true }} />);
  expect(scroll.scrollTop).toBe(28);
});
