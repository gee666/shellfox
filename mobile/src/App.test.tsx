// @vitest-environment jsdom
import React, { type ReactNode } from 'react';
import { act, cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import type { OpenSession } from './SessionsScreen';
import type { SessionScreen } from './SessionScreen';
import type { FilesScreen } from './FilesScreen';
import App from '../App';

const mocks = vi.hoisted(() => ({ useModel: vi.fn(), nextIntent: 0, tabs: new Map<string, () => void>() }));
type Children = { children?: ReactNode };
vi.mock('uuid', () => ({ v4: () => `intent-${++mocks.nextIntent}` }));
vi.mock('react-native', () => ({
  BackHandler: { addEventListener: () => ({ remove: vi.fn() }) },
  StatusBar: () => null,
  View: ({ children }: Children) => <div>{children}</div>,
  Text: ({ children }: Children) => <span>{children}</span>,
  Pressable: ({ children, onPress, testID }: Children & { onPress: () => void; testID?: string }) => {
    if (testID) mocks.tabs.set(testID, onPress);
    return <button onClick={onPress}>{children}</button>;
  },
}));
vi.mock('react-native-safe-area-context', () => ({
  SafeAreaProvider: ({ children }: Children) => <div>{children}</div>,
  SafeAreaView: ({ children }: Children) => <div>{children}</div>,
}));
vi.mock('./useShellfox', () => ({ useShellfox: mocks.useModel }));
vi.mock('./theme', () => ({
  ThemeProvider: ({ children }: Children) => <div>{children}</div>,
  useTheme: () => ({ palette: { scheme: 'dark' } }),
}));
vi.mock('./ui', () => ({ useUi: () => ({ colors: {}, styles: {} }), Button: () => null, Note: () => null }));
vi.mock('./updates/useUpdates', () => ({ useUpdates: () => ({}) }));
vi.mock('./Prompt', () => ({ Prompt: () => null }));
vi.mock('./SettingsScreen', () => ({ SettingsScreen: () => <div>Settings screen</div> }));
vi.mock('./FilesScreen', () => ({ FilesScreen: ({ sessionId, tabId, onClose }: Parameters<typeof FilesScreen>[0]) =>
  <div data-testid="files" data-session={sessionId} data-tab={tabId}><button onClick={onClose}>Close Files</button></div> }));
vi.mock('./SessionsScreen', () => ({ SessionsScreen: ({ open, openFiles }: { open: (value: OpenSession) => void; openFiles: (peerId: string) => void }) => <>
  <button onClick={() => openFiles('host')}>Host files</button>
  <button onClick={() => open({ peerId: 'host', sessionId: 'session', takeControl: true })}>Tap session</button>
  <button onClick={() => open({ peerId: 'host', sessionId: 'session' })}>Restore selection</button>
</> }));
vi.mock('./SessionScreen', () => ({ SessionScreen: ({ controlIntent, onControlIntentCaptured, back, openFiles }: Parameters<typeof SessionScreen>[0]) => {
  const [captured] = React.useState(controlIntent);
  React.useEffect(() => { if (controlIntent) onControlIntentCaptured?.(controlIntent.id); }, [controlIntent, onControlIntentCaptured]);
  return <>
    <div data-testid="session" data-intent={captured?.id} data-connection={captured?.connectionId} />
    <button onClick={back}>Back</button>
    <button onClick={() => openFiles({ sessionId: 'session', tabId: 'tab' })}>Session files</button>
  </>;
} }));
function model(connectionId = 'original', hasSession = false) {
  return {
    ready: true, preferences: {}, machines: [{ peer: { id: 'host' }, state: { status: 'connected', connectionId }, client: { files: {} }, snapshot: { sessions: hasSession ? [{ id: 'session', tabs: [{ id: 'tab' }] }] : [] }, snapshotConnectionId: connectionId }],
    error: null,
  };
}
beforeEach(() => { vi.clearAllMocks(); mocks.nextIntent = 0; mocks.tabs.clear(); mocks.useModel.mockReturnValue(model()); });
afterEach(cleanup);
it('opens host Files without any session', () => {
  render(<App />); fireEvent.click(screen.getByText('Host files'));
  expect(screen.getByTestId('files').getAttribute('data-session')).toBeNull();
  expect(screen.getByTestId('files').getAttribute('data-tab')).toBeNull();
});
it('keeps the App-owned Files screen mounted when its original session disappears', () => {
  mocks.useModel.mockReturnValue(model('original', true));
  const view = render(<App />); fireEvent.click(screen.getByText('Tap session')); fireEvent.click(screen.getByText('Session files'));
  expect(screen.getByTestId('files').getAttribute('data-session')).toBe('session');
  mocks.useModel.mockReturnValue(model('original', false)); view.rerender(<App />);
  expect(screen.getByTestId('files')).toBeTruthy();
  fireEvent.click(screen.getByText('Back')); expect(screen.getByTestId('files')).toBeTruthy();
});
it('creates connection-scoped takeover intent only for explicit session navigation', () => {
  const view = render(<App />);
  fireEvent.click(screen.getByText('Tap session'));
  expect(screen.getByTestId('session').getAttribute('data-intent')).toBe('intent-1');
  expect(screen.getByTestId('session').getAttribute('data-connection')).toBe('original');
  view.rerender(<App />);
  expect(screen.getByTestId('session').getAttribute('data-intent')).toBe('intent-1');
  mocks.useModel.mockReturnValue(model('replacement')); view.rerender(<App />);
  expect(screen.getByTestId('session').getAttribute('data-intent')).toBe('intent-1');
  expect(screen.getByTestId('session').getAttribute('data-connection')).toBe('original');
  fireEvent.click(screen.getByText('Back')); fireEvent.click(screen.getByText('Tap session'));
  expect(screen.getByTestId('session').getAttribute('data-intent')).toBe('intent-2');
  expect(screen.getByTestId('session').getAttribute('data-connection')).toBe('replacement');
});
it('clears captured intent from navigation state so a Settings round trip cannot retake control', () => {
  render(<App />); fireEvent.click(screen.getByText('Tap session'));
  expect(screen.getByTestId('session').getAttribute('data-intent')).toBe('intent-1');
  act(() => mocks.tabs.get('tab-settings')?.());
  expect(screen.getByText('Settings screen')).toBeTruthy();
  act(() => mocks.tabs.get('tab-sessions')?.());
  expect(screen.getByTestId('session').getAttribute('data-intent')).toBeNull();
});
it('does not interpret restoration or navigation without an explicit focus action as takeover', () => {
  render(<App />); fireEvent.click(screen.getByText('Restore selection'));
  expect(screen.getByTestId('session').getAttribute('data-intent')).toBeNull();
});
it('does not carry an offline session tap across a later connection', () => {
  const offline = model(); offline.machines[0].state.status = 'reconnecting';
  mocks.useModel.mockReturnValue(offline);
  const view = render(<App />); fireEvent.click(screen.getByText('Tap session'));
  expect(screen.getByTestId('session').getAttribute('data-intent')).toBeNull();
  mocks.useModel.mockReturnValue(model('replacement')); view.rerender(<App />);
  expect(screen.getByTestId('session').getAttribute('data-intent')).toBeNull();
});
