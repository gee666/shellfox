// @vitest-environment jsdom
import React, { type ReactNode } from 'react';
import '@testing-library/jest-dom/vitest';
import { act, cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { ShellfoxModel } from './useShellfox';
import { defaultPreferences } from './preferences';

const native = vi.hoisted(() => ({ openURL: vi.fn(), appState: { currentState: 'active' as string | null } }));
vi.mock('react-native', () => ({
  Alert: { alert: vi.fn() }, AppState: native.appState, Linking: { openURL: native.openURL }, Platform: { OS: 'ios' },
  ScrollView: ({ children }: { children?: ReactNode }) => <div>{children}</div>,
  Text: ({ children }: { children?: ReactNode }) => <span>{children}</span>,
  View: ({ children }: { children?: ReactNode }) => <div>{children}</div>,
}));
vi.mock('./ui', () => ({
  useUi: () => ({ colors: {}, styles: {} }),
  Button: ({ title, disabled, onPress }: { title: string; disabled?: boolean; onPress: () => void }) => <button disabled={disabled} onClick={onPress}>{title}</button>,
  Card: ({ children }: { children?: ReactNode }) => <div>{children}</div>,
  Note: ({ children }: { children?: ReactNode }) => <span>{children}</span>,
  Field: () => null,
}));
vi.mock('./updates/native', () => ({ updates: {} }));
vi.mock('./preferences', () => ({ defaultPreferences: { hostname: 'shellfox-mobile', remembered: [], fontSize: 14, accentColor: '#ec4899', backgroundColor: '#111016' } }));
vi.mock('./ColorSwatches', () => ({ ColorSwatches: () => null }));
const { SettingsScreen } = await import('./SettingsScreen');
const valid = 'https://login.tailscale.com/a/test-only';
function setup(loginUrl: string | null = valid, returnedUrl: string | null = valid, click = true) {
  const model: ShellfoxModel = {
    ready: true, loading: false, preferences: defaultPreferences,
    network: { status: 'login-required', loginUrl, error: null }, peers: [], machines: [], error: null,
    setError: vi.fn(), report: vi.fn(), updatePreferences: vi.fn(), connect: vi.fn(), disconnect: vi.fn(),
    login: vi.fn(async () => ({ status: 'login-required', loginUrl: returnedUrl, error: null })), logout: vi.fn(),
    renameDevice: vi.fn(), refresh: vi.fn(), retryStart: vi.fn(),
  };
  const view = render(<SettingsScreen model={model} prompt={vi.fn()} />);
  const rerender = () => view.rerender(<SettingsScreen model={model} prompt={vi.fn()} />);
  if (click) fireEvent.click(screen.getByRole('button', { name: /sign in.*browser/i }));
  return { ...model, rerender, unmount: view.unmount, source: model };
}
beforeEach(() => { vi.resetAllMocks(); native.openURL.mockResolvedValue(undefined); native.appState.currentState = 'active'; });
afterEach(() => { cleanup(); vi.useRealTimers(); });
describe('explicit browser sign-in', () => {
  it.each(['active', 'inactive', 'background', null])('opens the validated URL even when AppState is %s', async state => {
    native.appState.currentState = state;
    const model = setup();
    await waitFor(() => expect(native.openURL).toHaveBeenCalledWith(valid));
    expect(model.login).not.toHaveBeenCalled();
    expect(model.report).not.toHaveBeenCalled();
  });
  it('does not launch a URL merely because settings opens', () => {
    setup(valid, valid, false);
    expect(native.openURL).not.toHaveBeenCalled();
  });
  it('times out a pending URL request and re-enables sign-in', async () => {
    vi.useFakeTimers();
    const model = setup(null, null);
    await act(async () => { await Promise.resolve(); });
    await act(async () => { await vi.advanceTimersByTimeAsync(30000); });
    expect(model.report).toHaveBeenCalledWith(expect.objectContaining({ message: 'Sign-in timed out. Try again.' }));
    expect(screen.getByRole('button', { name: 'Sign in with browser' })).not.toBeDisabled();
    expect(native.openURL).not.toHaveBeenCalled();
  });
  it('waits for native login and opens its returned URL', async () => {
    const model = setup(null);
    await waitFor(() => expect(native.openURL).toHaveBeenCalledWith(valid));
    expect(model.login).toHaveBeenCalledOnce();
  });
  it('waits for a later polled URL and opens once, not on startup or subsequent polls', async () => {
    const model = setup(null, null);
    await waitFor(() => expect(model.login).toHaveBeenCalledOnce());
    expect(screen.getByRole('button', { name: 'Opening browser...' })).toBeDisabled();
    expect(native.openURL).not.toHaveBeenCalled();
    model.source.network = { ...model.source.network, loginUrl: valid };
    model.rerender();
    await waitFor(() => expect(native.openURL).toHaveBeenCalledOnce());
    model.rerender();
    expect(native.openURL).toHaveBeenCalledOnce();
  });
  it('does not open a late URL after leaving settings', async () => {
    const model = setup(null, null);
    await waitFor(() => expect(model.login).toHaveBeenCalledOnce());
    model.unmount();
    model.source.network = { ...model.source.network, loginUrl: valid };
    expect(native.openURL).not.toHaveBeenCalled();
  });
  it('reports a native error while waiting for a URL and re-enables sign-in', async () => {
    const model = setup(null, null);
    await waitFor(() => expect(model.login).toHaveBeenCalledOnce());
    model.source.network = { ...model.source.network, error: 'Login failed' };
    model.rerender();
    await waitFor(() => expect(model.report).toHaveBeenCalledWith(expect.objectContaining({ message: 'Login failed' })));
    expect(native.openURL).not.toHaveBeenCalled();
    expect(screen.getByRole('button', { name: 'Sign in with browser' })).not.toBeDisabled();
  });
  it.each(['http://login.tailscale.com/a/test', 'https://evil.test/a/test', 'https://user:pass@login.tailscale.com/a/test'])('rejects an unsafe sign-in URL %s', async url => {
    const model = setup(url);
    await waitFor(() => expect(model.report).toHaveBeenCalled());
    expect(native.openURL).not.toHaveBeenCalled();
  });
  it('reports launch failure instead of silently ignoring the tap', async () => {
    native.openURL.mockRejectedValue(new Error('No browser available'));
    const model = setup();
    await waitFor(() => expect(model.report).toHaveBeenCalledWith(expect.objectContaining({ message: 'No browser available' })));
  });
});
