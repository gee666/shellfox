// @vitest-environment jsdom
import '@testing-library/jest-dom/vitest';
import './terminal-test-mocks';
import { StrictMode, useRef } from 'react';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { act, cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { success } from '../shared/contracts';
import type { SessionDto } from '../shared/contracts';
import { App } from './App';
import { SessionSidebar } from './SessionSidebar';
import { createManagerClient } from './store';
import type { ManagerClient } from './store';
import { mockApi, session, snapshot } from './test-fixtures';
import { useVisibleSessions } from './visible-sessions';

const clients: ManagerClient[] = [];
const observers: TestIntersectionObserver[] = [];
let hidden = false;
let height = 180;
let terminalTop = 0;
const resizeCallbacks = new Set<() => void>();
function rect(top: number, height: number, left = 0, width = 220): DOMRect {
  return { top, bottom: top + height, left, right: left + width, width, height, x: left, y: top, toJSON() {} };
}
// jsdom does not implement layout or IntersectionObserver. Model clipping to
// the window and the single sidebar scrollbar, including zero-area edge hits.
class TestIntersectionObserver {
  targets = new Set<Element>();
  constructor(private callback: IntersectionObserverCallback, readonly options: IntersectionObserverInit) { observers.push(this); }
  observe(target: Element) { this.targets.add(target); }
  unobserve(target: Element) { this.targets.delete(target); }
  disconnect() { this.targets.clear(); }
  takeRecords() { return []; }
  deliver() {
    const entries = [...this.targets].map(target => {
      const bounds = target.getBoundingClientRect();
      const scroll = target.closest('.sidebar-scroll');
      const clip = scroll?.getBoundingClientRect() ?? rect(0, 600, 0, 800);
      const width = Math.min(bounds.right, clip.right, 800) - Math.max(bounds.left, clip.left, 0);
      const height = Math.min(bounds.bottom, clip.bottom, 600) - Math.max(bounds.top, clip.top, 0);
      return { target, isIntersecting: width >= 0 && height >= 0, intersectionRatio: Math.max(0, width) * Math.max(0, height) / (bounds.width * bounds.height || 1) } as IntersectionObserverEntry;
    });
    this.callback(entries, this as unknown as IntersectionObserver);
  }
}
const frame = async () => { await act(async () => { await new Promise<void>(resolve => requestAnimationFrame(() => resolve())); }); };
async function deliver() { act(() => observers.forEach(observer => observer.deliver())); await frame(); }
function ids(api: ReturnType<typeof mockApi>['api']) { return api.setVisibleSessions.mock.lastCall?.[0].sessionIds; }
function sorted(items: SessionDto[]) { return items.map(item => item.id).sort(); }

beforeEach(() => {
  hidden = false; height = 180; terminalTop = 0; localStorage.clear();
  vi.spyOn(document, 'hidden', 'get').mockImplementation(() => hidden);
  vi.stubGlobal('IntersectionObserver', TestIntersectionObserver);
  vi.stubGlobal('ResizeObserver', class {
    constructor(private callback: () => void) { resizeCallbacks.add(callback); }
    observe() {}
    disconnect() { resizeCallbacks.delete(this.callback); }
  });
  vi.spyOn(HTMLElement.prototype, 'clientHeight', 'get').mockImplementation(function (this: HTMLElement) {
    return this.classList.contains('sidebar-scroll') ? height : 0;
  });
  vi.spyOn(HTMLElement.prototype, 'getBoundingClientRect').mockImplementation(function (this: HTMLElement) {
    if (this.classList.contains('sidebar-scroll')) return rect(0, height);
    if (this.classList.contains('terminal-panel')) return rect(terminalTop, 600, 220, 580);
    const list = this.getAttribute('role') === 'list' ? this : this.parentElement;
    if (list?.getAttribute('role') !== 'list') return rect(0, 0);
    const scroll = list.closest('.sidebar-scroll')!;
    const liveCount = Number(scroll.querySelector('[aria-label="Sessions"] [role="listitem"]')?.getAttribute('aria-setsize') ?? 0);
    const archiveOffset = list.parentElement?.getAttribute('aria-label') === 'Archived sessions' ? liveCount * 36 + 32 : 0;
    const top = 4 + archiveOffset - scroll.scrollTop;
    return this === list ? rect(top, Number(list.querySelector('[role="listitem"]')?.getAttribute('aria-setsize')) * 36)
      : rect(top + (Number(this.getAttribute('aria-posinset')) - 1) * 36, 36);
  });
});
afterEach(() => {
  cleanup(); clients.splice(0).forEach(client => client.stop());
  observers.splice(0); resizeCallbacks.clear(); vi.restoreAllMocks(); vi.unstubAllGlobals();
});
function Sidebar({ client, items }: { client: ManagerClient; items: SessionDto[] }) {
  const root = useRef<HTMLDivElement>(null);
  useVisibleSessions(client.api, root);
  return <div ref={root}><SessionSidebar client={client} sessions={items} selectedId={null} busy={{}} invalidation={0} pageSize={100} /></div>;
}
function sidebar(items: SessionDto[]) {
  const fixture = mockApi(snapshot(items));
  const client = createManagerClient(fixture.api); clients.push(client);
  const view = render(<Sidebar client={client} items={items} />);
  const scroll = view.container.querySelector<HTMLDivElement>('.sidebar-scroll')!;
  return { ...view, fixture, client, scroll };
}

it('reports partial viewport rows, excluding overscan, pinned, boundary and menu-retained offscreen rows', async () => {
  const items = Array.from({ length: 1000 }, (_, index) => session(index + 1));
  items[0]!.pinnedAt = '2026-10-04T11:00:00.000Z';
  const { fixture, scroll, container } = sidebar(items);
  expect(observers[0]!.options).toEqual({ root: null, rootMargin: '0px', threshold: [0, Number.EPSILON] });
  fireEvent.contextMenu(screen.getByRole('button', { name: 'Select session Session 2' }));
  scroll.scrollTop = 500 * 36 + 4 + 18;
  fireEvent.scroll(scroll);
  await waitFor(() => expect(screen.getByRole('button', { name: 'Select session Session 501' })).toBeInTheDocument());
  await deliver();
  expect(ids(fixture.api)).toEqual(sorted(items.slice(500, 506)));
  // They remain mounted for keyboard/menu behavior, but are not reported.
  for (const index of [0, 1, 499, 506, 999]) expect(container.querySelector(`[data-session-id="${items[index]!.id}"]`)).not.toBeNull();
  expect(fixture.api.setVisibleSessions).toHaveBeenCalledTimes(2); // Initial empty + measured set.
  await deliver();
  expect(fixture.api.setVisibleSessions).toHaveBeenCalledTimes(2);
});

it('uses geometry for small lists, excludes edge-only intersections and updates after resize, deletion and reorder', async () => {
  height = 72;
  const items = Array.from({ length: 10 }, (_, index) => session(index + 1));
  const { fixture, scroll, rerender, client } = sidebar(items);
  scroll.scrollTop = 4;
  await deliver();
  expect(ids(fixture.api)).toEqual(sorted(items.slice(0, 2))); // Row 3 only touches the bottom edge.
  height = 108;
  act(() => resizeCallbacks.forEach(callback => callback()));
  await deliver();
  expect(ids(fixture.api)).toEqual(sorted(items.slice(0, 3)));
  rerender(<Sidebar client={client} items={[items[4]!, items[3]!, ...items.slice(5)]} />);
  await act(async () => {}); // MutationObserver removes obsolete targets and observes replacements.
  await deliver();
  expect(ids(fixture.api)).toEqual(sorted([items[4]!, items[3]!, items[5]!]));
});

it('observes loaded archive rows only when expanded and intersecting the shared scrollbar', async () => {
  height = 72;
  const items = [session(1)];
  const history = Array.from({ length: 6 }, (_, index) => session(index + 20, { status: 'settled', settledAt: '2026-10-04T11:00:00.000Z' }));
  const { fixture, scroll } = sidebar(items);
  fixture.api.getHistory.mockImplementation(async query => success({ items: history, total: history.length, page: query.page, pageSize: query.pageSize }));
  fireEvent.click(screen.getByRole('button', { name: /Archived/ }));
  await screen.findByRole('button', { name: 'Select session Session 20' });
  scroll.scrollTop = 4 + 36 + 32;
  fireEvent.scroll(scroll);
  await deliver();
  expect(ids(fixture.api)).toEqual(sorted(history.slice(0, 2)));
  fireEvent.click(screen.getByRole('button', { name: /Archived/ }));
  await frame();
  expect(ids(fixture.api)).toEqual([]);
});

it('clears immediately while hidden, ignores hidden deliveries and resumes with fresh intersections', async () => {
  const { fixture, scroll, unmount } = sidebar([session(1), session(2)]);
  await deliver();
  expect(ids(fixture.api)).toEqual(sorted([session(1), session(2)]));
  // A pending frame must not send visible IDs after the hide notification.
  act(() => observers[0]!.deliver());
  hidden = true;
  fireEvent(document, new Event('visibilitychange'));
  expect(ids(fixture.api)).toEqual([]);
  const calls = fixture.api.setVisibleSessions.mock.calls.length;
  scroll.scrollTop = 1000;
  await deliver();
  expect(fixture.api.setVisibleSessions).toHaveBeenCalledTimes(calls);
  hidden = false;
  fireEvent(document, new Event('visibilitychange'));
  await deliver();
  expect(ids(fixture.api)).toEqual([]);
  scroll.scrollTop = 4;
  await deliver();
  expect(ids(fixture.api)).toEqual(sorted([session(1), session(2)]));
  unmount();
  expect(ids(fixture.api)).toEqual([]);
  expect(observers[0]!.targets.size).toBe(0);
});

it('starts hidden without observing rows or sending nonempty IDs', async () => {
  hidden = true;
  const { fixture } = sidebar([session(1)]);
  await deliver();
  expect(fixture.api.setVisibleSessions).toHaveBeenCalledExactlyOnceWith({ sessionIds: [] });
  expect(observers[0]!.targets.size).toBe(0);
  hidden = false;
  fireEvent(document, new Event('visibilitychange'));
  await deliver();
  expect(ids(fixture.api)).toEqual([session(1).id]);
});

it('App unions the separate visible terminal, deduplicates its row and removes it for Settings or selection changes', async () => {
  height = 72;
  const items = Array.from({ length: 10 }, (_, index) => session(index + 1));
  const fixture = mockApi(snapshot(items));
  const { container } = render(<App api={fixture.api} />);
  await screen.findByRole('button', { name: 'Select session Session 1' });
  const scroll = container.querySelector<HTMLDivElement>('.sidebar-scroll')!;
  scroll.scrollTop = 4;
  await deliver();
  expect(ids(fixture.api)).toEqual(sorted(items.slice(0, 2)));
  scroll.scrollTop = 4 + 5 * 36;
  fireEvent.scroll(scroll);
  await deliver();
  expect(ids(fixture.api)).toEqual(sorted([items[0]!, items[5]!, items[6]!]));
  fireEvent.click(screen.getByRole('button', { name: 'Settings' }));
  await frame();
  await deliver();
  expect(ids(fixture.api)).toEqual(sorted(items.slice(5, 7)));
  fireEvent.click(screen.getByRole('button', { name: 'Settings' }));
  await frame();
  await deliver();
  expect(ids(fixture.api)).toEqual(sorted([items[0]!, items[5]!, items[6]!]));
  terminalTop = 700;
  await deliver();
  expect(ids(fixture.api)).toEqual(sorted(items.slice(5, 7)));
  terminalTop = 0;
  fireEvent.click(screen.getByRole('button', { name: 'Select session Session 7' }));
  await frame();
  await deliver();
  expect(ids(fixture.api)).toEqual(sorted(items.slice(5, 7)));
});

it('does not report a selected legacy placeholder as a visible terminal', async () => {
  height = 36;
  const items = [session(1, { adapterId: 'windows-terminal' }), session(2), session(3)];
  const fixture = mockApi(snapshot(items));
  const { container } = render(<App api={fixture.api} />);
  await screen.findByRole('button', { name: 'Select session Session 1' });
  const scroll = container.querySelector<HTMLDivElement>('.sidebar-scroll')!;
  scroll.scrollTop = 4 + 36;
  fireEvent.scroll(scroll);
  await deliver();
  expect(ids(fixture.api)).toEqual([items[1]!.id]);
  expect(container.querySelector('[data-terminal-session-id]')).toBeNull();
});

it('does not keep a selected archived placeholder in the union after its archive row is collapsed', async () => {
  const live = session(1);
  const archived = session(20, { status: 'settled', settledAt: '2026-10-04T11:00:00.000Z' });
  const fixture = mockApi(snapshot([live]));
  fixture.api.getHistory.mockImplementation(async query => success({ items: [archived], total: 1, page: query.page, pageSize: query.pageSize }));
  const { container } = render(<App api={fixture.api} />);
  await screen.findByRole('button', { name: 'Select session Session 1' });
  fireEvent.click(screen.getByRole('button', { name: /Archived/ }));
  fireEvent.click(await screen.findByRole('button', { name: 'Select session Session 20' }));
  expect(await screen.findByText('Archived session')).toBeVisible();
  await frame();
  await deliver();
  expect(ids(fixture.api)).toContain(archived.id); // Its row is visible, not a terminal.
  expect(container.querySelector('[data-terminal-session-id]')).toBeNull();
  fireEvent.click(screen.getByRole('button', { name: /Archived/ }));
  await frame();
  await deliver();
  expect(ids(fixture.api)).toEqual([live.id]);
});

it('cleans up StrictMode observers and does not retry or toast rejected background configuration', async () => {
  const fixture = mockApi(snapshot([session(1)]));
  fixture.api.setVisibleSessions.mockRejectedValue(new Error('Disconnected'));
  const client = createManagerClient(fixture.api); clients.push(client);
  const { unmount } = render(<StrictMode><Sidebar client={client} items={[session(1)]} /></StrictMode>);
  await deliver();
  expect(ids(fixture.api)).toEqual([session(1).id]);
  expect(observers.filter(observer => observer.targets.size)).toHaveLength(1);
  const calls = fixture.api.setVisibleSessions.mock.calls.length;
  await deliver();
  expect(fixture.api.setVisibleSessions).toHaveBeenCalledTimes(calls);
  expect(screen.queryByRole('alert')).not.toBeInTheDocument();
  unmount();
  expect(ids(fixture.api)).toEqual([]);
  expect(observers.every(observer => observer.targets.size === 0)).toBe(true);
});
