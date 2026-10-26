// @vitest-environment jsdom
import React from 'react';
import { act, cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { SessionDto } from '../../src/shared/contracts';
import { failure, success } from '../../src/shared/contracts';
import { session, snapshot } from '../../src/renderer/test-fixtures';
import { derivePalette } from '../../src/renderer/theme';
import { RemoteClient } from './client/client';
import { TestTransport } from './client/test-fixtures';
import type { MachineView, ShellfoxModel } from './useShellfox';
import type { PromptConfig } from './Prompt';
import { SessionsScreen } from './SessionsScreen';
import { StatusIndicator } from './StatusIndicator';
import { ThemeProvider } from './theme';

type NativeProps = {
  children?: React.ReactNode; style?: unknown; accessibilityLabel?: string; accessibilityRole?: string;
  accessibilityState?: { expanded?: boolean; disabled?: boolean }; testID?: string; disabled?: boolean;
  onPress?: () => void; onLongPress?: () => void;
};
type ListProps = NativeProps & {
  sections: { key: string; machine: MachineView; data: SessionDto[] }[];
  renderSectionHeader: (value: { section: ListProps['sections'][number] }) => React.ReactNode;
  renderItem: (value: { item: SessionDto; section: ListProps['sections'][number] }) => React.ReactNode;
  ListEmptyComponent: React.ReactNode;
  onViewableItemsChanged: (value: { viewableItems: { item: unknown; index?: number | null; isViewable: boolean; section?: unknown }[] }) => void;
};
let listProps: ListProps | undefined;
function flatten(style: unknown): object {
  if (typeof style === 'function') return flatten(style({ pressed: false }));
  if (Array.isArray(style)) return Object.assign({}, ...style.map(flatten));
  return style && typeof style === 'object' ? style : {};
}
function nativeElement(tag: 'div' | 'button' | 'span', props: NativeProps) {
  return React.createElement(tag, {
    'aria-label': props.accessibilityLabel,
    role: props.accessibilityRole === 'image' ? 'img' : props.accessibilityRole,
    'aria-expanded': props.accessibilityState?.expanded,
    'data-testid': props.testID,
    'data-style': JSON.stringify(flatten(props.style)),
    disabled: props.disabled,
    onClick: props.onPress,
    onContextMenu: props.onLongPress,
  }, props.children);
}
const animation = vi.hoisted(() => ({
  start: vi.fn(), stop: vi.fn(), timing: vi.fn(), setValue: vi.fn(),
  interpolate: vi.fn(({ outputRange }: { inputRange: number[]; outputRange: number[] }) => outputRange[0]),
}));
vi.mock('react-native-safe-area-context', () => ({ useSafeAreaInsets: () => ({ top: 0, bottom: 0 }) }));
vi.mock('./terminal/useKeyboardOverlap', () => ({ useKeyboardOverlap: () => ({ keyboardVisible: false, keyboardInset: 0, keyboardOffset: 0, measureKeyboardInset: vi.fn() }) }));
vi.mock('react-native', () => ({
  View: (props: NativeProps) => nativeElement('div', props),
  Text: (props: NativeProps) => nativeElement('span', props),
  Pressable: (props: NativeProps) => nativeElement(props.accessibilityRole === 'button' ? 'button' : 'div', props),
  Modal: (props: NativeProps) => nativeElement('div', props),
  ScrollView: (props: NativeProps) => nativeElement('div', props),
  KeyboardAvoidingView: (props: NativeProps) => nativeElement('div', props),
  Platform: { OS: 'android' }, Alert: { alert: vi.fn() },
  StyleSheet: { create: <T,>(styles: T) => styles },
  TextInput: (props: NativeProps & { value: string; placeholder?: string; onChangeText: (value: string) => void }) => (
    <input aria-label={props.accessibilityLabel} placeholder={props.placeholder} value={props.value} onChange={event => props.onChangeText(event.target.value)} />
  ),
  SectionList: (props: ListProps) => {
    listProps = props;
    return nativeElement('div', { ...props, children: props.sections.length ? props.sections.map(section => (
      <React.Fragment key={section.machine.peer.id}>
        {props.renderSectionHeader({ section })}
        {section.data.map(item => <React.Fragment key={item.id}>{props.renderItem({ item, section })}</React.Fragment>)}
      </React.Fragment>
    )) : props.ListEmptyComponent });
  },
  Animated: {
    Value: class {
      constructor(_initial: number) {}
      setValue = animation.setValue;
      interpolate = animation.interpolate;
    },
    View: (props: NativeProps) => nativeElement('div', props),
    timing: (_value: unknown, config: unknown) => { animation.timing(config); return {}; },
    sequence: () => ({}), loop: () => ({ start: animation.start, stop: animation.stop }),
  },
  Easing: { ease: 'ease', linear: 'linear', inOut: (value: unknown) => value },
}));

function fixture(machines: MachineView[] = []): ShellfoxModel {
  return {
    ready: true, loading: false, network: { status: 'online', loginUrl: null, error: null },
    peers: [], machines, preferences: { hostname: 'phone', remembered: [], fontSize: 14 }, error: null,
    setError: vi.fn(), report: vi.fn(), updatePreferences: vi.fn(async () => {}),
    connect: vi.fn(async () => {}), disconnect: vi.fn(async () => {}),
    login: vi.fn(async () => ({ status: 'online', loginUrl: null, error: null })),
    logout: vi.fn(async () => {}), renameDevice: vi.fn(async () => {}), refresh: vi.fn(async () => {}), retryStart: vi.fn(async () => {}),
  };
}
function machine(items: SessionDto[] = [session()], id = 'host'): MachineView {
  const client = new RemoteClient(id, new TestTransport(), { createClientId: () => '10000000-0000-4000-8000-000000000001' });
  vi.spyOn(client, 'request').mockResolvedValue(success({ configured: true }));
  return {
    peer: { id, hostname: 'workstation', addresses: [], online: true }, client,
    state: { peerId: id, status: 'connected', connectionId: 'connection-1', lastError: null },
    snapshot: snapshot(items), snapshotConnectionId: 'connection-1', busyTabIds: {},
  };
}
const open = vi.fn();
const prompt = vi.fn<(value: PromptConfig) => void>();
const settings = vi.fn();
const ui = (model: ShellfoxModel) => <SessionsScreen model={model} open={open} prompt={prompt} settings={settings} />;
function visible(items: SessionDto[], peerId = 'host') {
  act(() => listProps?.onViewableItemsChanged({ viewableItems: items.map((item, index) => ({ item, index, isViewable: true, section: { key: peerId } })) }));
}
beforeEach(() => { vi.clearAllMocks(); listProps = undefined; });
afterEach(cleanup);

describe('compact Sessions screen', () => {
  it('waits for startup before offering Settings, without empty-state prose', () => {
    const model = fixture();
    const view = render(ui({ ...model, loading: true }));
    expect(screen.getByRole('progressbar', { name: 'Loading sessions' })).toBeTruthy();
    expect(screen.queryByRole('button', { name: 'Open Settings' })).toBeNull();
    view.rerender(ui(model));
    expect(screen.queryByRole('progressbar')).toBeNull();
    expect(screen.getByLabelText('Sessions list')).toBeTruthy();
    expect(screen.getByLabelText('No sessions')).toBeTruthy();
    expect(screen.queryByText(/Connect to your first machine|Your machines|No sessions yet/)).toBeNull();
    fireEvent.click(screen.getByRole('button', { name: 'Open Settings' }));
    expect(settings).toHaveBeenCalledOnce();
  });
  it('keeps machine name and connection, with compact separate 44px controls', () => {
    const host = machine(); render(ui(fixture([host])));
    expect(screen.getByText('workstation')).toBeTruthy(); expect(screen.getByLabelText('Connection connected')).toBeTruthy();
    expect(screen.queryByRole('button', { name: 'Machines' })).toBeNull();
    expect(screen.queryByText(/terminals|agents|waiting/)).toBeNull();
    const plus = screen.getByRole('button', { name: 'New session on workstation' });
    expect(plus.textContent).toBe('+'); expect(plus.getAttribute('data-style')).toContain('"borderRadius":22');
    for (const button of [plus, screen.getByRole('button', { name: 'Search sessions' }), screen.getByRole('button', { name: 'Manage Session 1' })]) {
      expect(button.getAttribute('data-style')).toContain('"width":44');
      expect(button.getAttribute('data-style')).toContain('"height":44');
    }
    const card = screen.getByRole('button', { name: 'Open Session 1' });
    expect(card.getAttribute('data-style')).toContain('"minHeight":44');
    expect(card.getAttribute('data-style')).toContain('"paddingVertical":4');
    expect(card.parentElement?.querySelectorAll('button')).toHaveLength(2);
    fireEvent.click(card); expect(open).toHaveBeenCalledWith({ peerId: 'host', sessionId: session().id });
  });
  it('hides search by default, searches title or path, and clears the filter when closed', () => {
    const host = machine([session(1), session(2, { cwd: '/home/OtherProject' })]); render(ui(fixture([host])));
    expect(screen.queryByRole('textbox')).toBeNull();
    const search = screen.getByRole('button', { name: 'Search sessions' }); fireEvent.click(search);
    expect(search.getAttribute('aria-expanded')).toBe('true');
    fireEvent.change(screen.getByRole('textbox', { name: 'Find a session' }), { target: { value: 'otherproject' } });
    expect(screen.queryByRole('button', { name: 'Open Session 1' })).toBeNull();
    expect(screen.getByRole('button', { name: 'Open Session 2' })).toBeTruthy();
    fireEvent.click(search); expect(screen.queryByRole('textbox')).toBeNull();
    expect(screen.getByRole('button', { name: 'Open Session 1' })).toBeTruthy();
  });
  it('shows three blue dots while a connected machine waits for its snapshot', () => {
    render(ui(fixture([{ ...machine(), snapshot: null }])));
    const loading = screen.getByRole('progressbar', { name: 'Loading sessions on workstation' });
    expect(loading.children).toHaveLength(3);
    for (const dot of loading.children) expect(dot.getAttribute('data-style')).toContain('#60a5fa');
    expect(screen.queryByText(/No sessions|Create one/)).toBeNull();
  });
  it('updates agent activity without visible status words', () => {
    const item = session(); item.tabs[0].status = 'running';
    const host = machine([item]); const model = fixture([host]); const view = render(ui(model));
    expect(screen.getByRole('img', { name: 'Agent idle' })).toBeTruthy();
    view.rerender(ui({ ...model, machines: [{ ...host, busyTabIds: { [item.tabs[0].id]: true } }] }));
    expect(screen.getByRole('img', { name: 'Agent working' })).toBeTruthy();
    expect(screen.queryByText(/Agent|running/)).toBeNull(); expect(animation.start).toHaveBeenCalledOnce();
  });
  it('creates through one OK action and retains the request ID after a failed submission', async () => {
    const host = machine(); let attempts = 0;
    vi.mocked(host.client.request).mockImplementation(async method => {
      if (method === 'getHomeDirectory') return success({ cwd: '/home/user' });
      if (method === 'completeDirectory') return success({ matches: [] });
      if (method === 'resolveDirectory') return success({ cwd: '/work' });
      if (method === 'createSession') return ++attempts === 1 ? failure('INTERNAL', 'Try again.') : success(session());
      return success({ configured: true });
    });
    const model = fixture([host]); render(ui(model));
    await act(async () => { fireEvent.click(screen.getByRole('button', { name: 'New session on workstation' })); });
    expect(prompt).not.toHaveBeenCalled();
    expect(screen.getByRole('textbox', { name: 'Host directory' })).toHaveProperty('value', '/home/user');
    fireEvent.change(screen.getByRole('textbox', { name: 'Host directory' }), { target: { value: '/work' } });
    await act(async () => { fireEvent.click(screen.getByRole('button', { name: 'OK' })); });
    expect(screen.getByText('Try again.')).toBeTruthy();
    await act(async () => { fireEvent.click(screen.getByRole('button', { name: 'OK' })); });
    const calls = vi.mocked(host.client.request).mock.calls.filter(([method]) => method === 'createSession');
    expect(calls).toHaveLength(2); expect(calls[0]).toEqual(calls[1]);
    expect(calls[0][1]).toMatchObject({ cwd: '/work', requestId: expect.any(String) });
    expect(host.client.request).not.toHaveBeenCalledWith('createDirectory', expect.anything());
    expect(model.refresh).toHaveBeenCalledWith(host.client);
    expect(open).toHaveBeenCalledWith({ peerId: 'host', sessionId: session().id });
    expect(screen.queryByRole('button', { name: 'OK' })).toBeNull();
  });
  it('confirms recursive folder creation and keeps one request ID across session retries', async () => {
    const host = machine(); let attempts = 0;
    vi.mocked(host.client.request).mockImplementation(async method => {
      if (method === 'getHomeDirectory') return success({ cwd: '/home/user' });
      if (method === 'completeDirectory') return success({ matches: [] });
      if (method === 'resolveDirectory') return failure('NOT_FOUND', 'Missing');
      if (method === 'createDirectory') return success({ cwd: '/home/user/new/child' });
      if (method === 'createSession') return ++attempts === 1 ? failure('INTERNAL', 'Retry session.') : success(session());
      return success({ configured: true });
    });
    render(ui(fixture([host])));
    await act(async () => { fireEvent.click(screen.getByRole('button', { name: 'New session on workstation' })); });
    fireEvent.change(screen.getByRole('textbox', { name: 'Host directory' }), { target: { value: '~/new/child' } });
    await act(async () => { fireEvent.click(screen.getByRole('button', { name: 'OK' })); });
    expect(host.client.request).not.toHaveBeenCalledWith('createDirectory', expect.anything());
    await act(async () => { fireEvent.click(screen.getByRole('button', { name: 'Yes' })); });
    expect(screen.getByText('Retry session.')).toBeTruthy();
    await act(async () => { fireEvent.click(screen.getByRole('button', { name: 'Yes' })); });
    const calls = vi.mocked(host.client.request).mock.calls.filter(([method]) => method === 'createSession');
    expect(calls).toHaveLength(2); expect(calls[0]).toEqual(calls[1]);
    expect(calls[0][1]).toMatchObject({ cwd: '/home/user/new/child', requestId: expect.any(String) });
    expect(host.client.request).toHaveBeenCalledWith('createDirectory', { path: '~/new/child' });
    expect(open).toHaveBeenCalledOnce();
  });
  it('does not open a stale session after the new-session modal is replaced', async () => {
    const host = machine(); let finish!: (value: ReturnType<typeof success<SessionDto>>) => void;
    const pending = new Promise<ReturnType<typeof success<SessionDto>>>(resolve => { finish = resolve; });
    vi.mocked(host.client.request).mockImplementation(async method => {
      if (method === 'getHomeDirectory') return success({ cwd: '/home/user' });
      if (method === 'completeDirectory') return success({ matches: [] });
      if (method === 'resolveDirectory') return success({ cwd: '/work' });
      if (method === 'createSession') return pending;
      return success({ configured: true });
    });
    const model = fixture([host]); render(ui(model));
    await act(async () => { fireEvent.click(screen.getByRole('button', { name: 'New session on workstation' })); });
    await act(async () => { fireEvent.click(screen.getByRole('button', { name: 'OK' })); });
    await act(async () => { fireEvent.click(screen.getByRole('button', { name: 'New session on workstation' })); });
    await act(async () => { finish(success(session())); });
    expect(open).not.toHaveBeenCalled(); expect(model.refresh).not.toHaveBeenCalled();
    expect(screen.getByRole('textbox', { name: 'Host directory' })).toHaveProperty('value', '/home/user');
    expect(screen.getByRole('button', { name: 'OK' })).toHaveProperty('disabled', false);
  });
  it('allocates a fresh request ID when opening another new-session dialog', async () => {
    const host = machine();
    vi.mocked(host.client.request).mockImplementation(async method => {
      if (method === 'getHomeDirectory') return success({ cwd: '/home/user' });
      if (method === 'completeDirectory') return success({ matches: [] });
      if (method === 'resolveDirectory') return success({ cwd: '/work' });
      if (method === 'createSession') return success(session());
      return success({ configured: true });
    });
    render(ui(fixture([host])));
    for (let i = 0; i < 2; i++) {
      await act(async () => { fireEvent.click(screen.getByRole('button', { name: 'New session on workstation' })); });
      await act(async () => { fireEvent.click(screen.getByRole('button', { name: 'OK' })); });
    }
    const calls = vi.mocked(host.client.request).mock.calls.filter(([method]) => method === 'createSession');
    expect(calls).toHaveLength(2); expect(calls[0][1]).not.toEqual(calls[1][1]);
  });
  it('retains the management menu, environment editor, and pin action', async () => {
    const host = machine(); const model = fixture([host]); render(ui(model));
    fireEvent.click(screen.getByRole('button', { name: 'Manage Session 1' }));
    for (const title of ['Rename', 'Environment', 'Pin', 'Settle session', 'Delete session...']) expect(screen.getByRole('button', { name: title })).toBeTruthy();
    await act(async () => { fireEvent.click(screen.getByRole('button', { name: 'Pin' })); });
    expect(host.client.request).toHaveBeenCalledWith('setSessionPinned', { sessionId: session().id, pinned: true });
    fireEvent.click(screen.getByRole('button', { name: 'Manage Session 1' }));
    fireEvent.click(screen.getByRole('button', { name: 'Environment' }));
    expect(screen.getByRole('button', { name: 'Add variable' })).toBeTruthy();
    await act(async () => { fireEvent.click(screen.getByRole('button', { name: 'Save' })); });
    expect(host.client.request).toHaveBeenCalledWith('setSessionEnv', { sessionId: session().id, env: [] });
  });
  it('keeps colliding session IDs scoped to their visible machine, ignoring section headers', () => {
    const first = machine(); const second = machine([session()], 'other-host');
    render(ui(fixture([first, second]))); visible([session()], second.peer.id);
    expect(first.client.request).toHaveBeenLastCalledWith('setVisibleSessions', { sessionIds: [] });
    expect(second.client.request).toHaveBeenLastCalledWith('setVisibleSessions', { sessionIds: [session().id] });
    act(() => listProps?.onViewableItemsChanged({ viewableItems: [{ item: session(), index: null, isViewable: true, section: { key: second.peer.id } }] }));
    expect(second.client.request).toHaveBeenLastCalledWith('setVisibleSessions', { sessionIds: [] });
  });
  it('reports only visible sessions, deduplicates busy updates, reapplies on reconnect, and clears on leave', () => {
    const host = machine([session(1), session(2)]); const model = fixture([host]); const view = render(ui(model));
    visible([session(1)]);
    expect(host.client.request).toHaveBeenLastCalledWith('setVisibleSessions', { sessionIds: [session(1).id] });
    const count = vi.mocked(host.client.request).mock.calls.length;
    view.rerender(ui({ ...model, machines: [{ ...host, busyTabIds: { irrelevant: true } }] }));
    expect(host.client.request).toHaveBeenCalledTimes(count);
    visible([session(2)]);
    expect(host.client.request).toHaveBeenLastCalledWith('setVisibleSessions', { sessionIds: [session(2).id] });
    view.rerender(ui({ ...model, machines: [{ ...host, state: { ...host.state, connectionId: 'connection-2' } }] }));
    expect(host.client.request).toHaveBeenLastCalledWith('setVisibleSessions', { sessionIds: [session(2).id] });
    expect(host.client.request).toHaveBeenCalledTimes(count + 2);
    view.unmount(); expect(host.client.request).toHaveBeenLastCalledWith('setVisibleSessions', { sessionIds: [] });
  });
});

describe('mobile status indicator', () => {
  it.each([['shell', '#60a5fa'], ['running', '#4ade80'], ['busy', '#4ade80'], ['settled', '#96734e'], ['error', '#fc8397']] as const)('uses desktop %s color', (kind, color) => {
    const view = render(<StatusIndicator kind={kind} label="Status" />);
    const indicator = screen.getByRole('img', { name: 'Status' });
    expect(indicator.children).toHaveLength(1); expect(indicator.children[0].getAttribute('data-style')).toContain(color);
    expect(animation.start).toHaveBeenCalledTimes(kind === 'busy' ? 1 : 0);
    view.unmount(); expect(animation.stop).toHaveBeenCalledTimes(kind === 'busy' ? 1 : 0);
  });
  it.each([
    ['shell', 'Shell', '--dot-shell'],
    ['running', 'Agent idle', '--dot-running'],
    ['busy', 'Agent working', '--dot-running'],
    ['settled', 'Settled', '--dot-settled'],
    ['error', 'Error', '--dot-error'],
  ] as const)('updates %s to the desktop light-theme color without changing its label or animation', (kind, label, variable) => {
    const view = render(<ThemeProvider><StatusIndicator kind={kind} label={label} /></ThemeProvider>);
    view.rerender(<ThemeProvider accentColor="#ffff00" backgroundColor="#ffffff"><StatusIndicator kind={kind} label={label} /></ThemeProvider>);
    const palette = derivePalette('#ffff00', '#ffffff');
    const dot = screen.getByRole('img', { name: label }).children[0];
    expect(dot.getAttribute('data-style')).toContain(`"backgroundColor":"${palette.vars[variable]}"`);
    expect(palette.vars[variable]).not.toBe(palette.accent);
    expect(animation.start).toHaveBeenCalledTimes(kind === 'busy' ? 1 : 0);
    expect(animation.stop).not.toHaveBeenCalled();
  });
  it('uses desktop shell blue for all loading dots after switching to a light theme', () => {
    const view = render(<ThemeProvider><StatusIndicator loading label="Loading sessions" /></ThemeProvider>);
    view.rerender(<ThemeProvider accentColor="#ffff00" backgroundColor="#ffffff"><StatusIndicator loading label="Loading sessions" /></ThemeProvider>);
    const loading = screen.getByRole('progressbar', { name: 'Loading sessions' });
    const palette = derivePalette('#ffff00', '#ffffff');
    expect(loading.children).toHaveLength(3);
    for (const dot of loading.children) expect(dot.getAttribute('data-style')).toContain(`"backgroundColor":"${palette.vars['--dot-shell']}"`);
    expect(animation.start).toHaveBeenCalledOnce();
    expect(animation.stop).not.toHaveBeenCalled();
  });
  it('loops the loading dots forward from left to right without reversing', () => {
    const view = render(<StatusIndicator loading />);
    expect(animation.timing).toHaveBeenCalledExactlyOnceWith({ toValue: 1, duration: 900, easing: 'linear', useNativeDriver: true, isInteraction: false });
    expect(animation.interpolate.mock.calls.map(([config]) => config)).toEqual([
      { inputRange: [0, 1 / 3, 2 / 3, 1], outputRange: [1, 0.25, 0.25, 1] },
      { inputRange: [0, 1 / 3, 2 / 3, 1], outputRange: [0.25, 1, 0.25, 0.25] },
      { inputRange: [0, 1 / 3, 2 / 3, 1], outputRange: [0.25, 0.25, 1, 0.25] },
    ]);
    view.unmount(); expect(animation.stop).toHaveBeenCalledOnce();
  });
  it('uses a non-blocking 1.2-second native animation for working agents and stops when idle', () => {
    const view = render(<StatusIndicator kind="busy" label="Agent working" />);
    expect(animation.timing).toHaveBeenCalledTimes(2);
    expect(animation.timing).toHaveBeenCalledWith(expect.objectContaining({ duration: 600, useNativeDriver: true, isInteraction: false }));
    expect(animation.interpolate).toHaveBeenCalledWith({ inputRange: [0, 1], outputRange: [1, 0.45] });
    expect(animation.interpolate).toHaveBeenCalledWith({ inputRange: [0, 1], outputRange: [1, 0.85] });
    view.rerender(<StatusIndicator kind="running" label="Agent idle" />); expect(animation.stop).toHaveBeenCalledOnce();
  });
});
