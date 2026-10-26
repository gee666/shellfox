// @vitest-environment jsdom
import React, { StrictMode, type ReactNode } from 'react';
import { act, cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { ManagerApi, Result } from '../../src/shared/contracts';
import { failure, success } from '../../src/shared/contracts';
import { DirectoryPrompt, type DirectoryPromptConfig } from './DirectoryPrompt';
import { createDirectoryFixtureApi } from '../e2e/DirectoryHarness';

type NativeProps = {
  children?: ReactNode; accessibilityLabel?: string; accessibilityRole?: string; disabled?: boolean;
  onPress?: () => void; onRequestClose?: () => void; keyboardShouldPersistTaps?: string;
  behavior?: string; keyboardVerticalOffset?: number; style?: unknown;
};
const keyboard = vi.hoisted(() => ({ keyboardVisible: true, keyboardInset: 120, keyboardOffset: 24, measureKeyboardInset: vi.fn() }));
vi.mock('react-native-safe-area-context', () => ({ useSafeAreaInsets: () => ({ top: 30, bottom: 20 }) }));
vi.mock('./terminal/useKeyboardOverlap', () => ({ useKeyboardOverlap: () => keyboard }));
vi.mock('react-native', () => ({
  Platform: { OS: 'android' }, StyleSheet: { create: <T,>(styles: T) => styles },
  View: ({ children, style }: NativeProps) => <div data-style={JSON.stringify(style)}>{children}</div>,
  Text: ({ children }: NativeProps) => <span>{children}</span>,
  Pressable: ({ children, accessibilityLabel, disabled, onPress }: NativeProps) => <button aria-label={accessibilityLabel} disabled={disabled} onClick={onPress}>{children}</button>,
  ScrollView: ({ children, keyboardShouldPersistTaps }: NativeProps) => <div data-taps={keyboardShouldPersistTaps}>{children}</div>,
  Modal: ({ children, onRequestClose }: NativeProps) => <div role="dialog" onKeyDown={event => { if (event.key === 'Escape') onRequestClose?.(); }}>{children}</div>,
  KeyboardAvoidingView: ({ children, behavior, keyboardVerticalOffset, style }: NativeProps) => <div data-testid="keyboard-layout" data-behavior={behavior} data-offset={keyboardVerticalOffset} data-style={JSON.stringify(style)}>{children}</div>,
  TextInput: ({ value, onChangeText, onSubmitEditing, editable, accessibilityLabel }: { value: string; onChangeText: (value: string) => void; onSubmitEditing: () => void; editable?: boolean; accessibilityLabel: string }) => (
    <input aria-label={accessibilityLabel} value={value} disabled={editable === false}
      onChange={event => onChangeText(event.target.value)} onKeyDown={event => { if (event.key === 'Enter') onSubmitEditing(); }} />
  ),
}));
function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>(done => { resolve = done; });
  return { promise, resolve };
}
function fixture(id = 'dialog-1') {
  const api = {
    getHomeDirectory: vi.fn<ManagerApi['getHomeDirectory']>().mockResolvedValue(success({ cwd: '/home/user' })),
    resolveDirectory: vi.fn<ManagerApi['resolveDirectory']>().mockResolvedValue(success({ cwd: '/resolved' })),
    createDirectory: vi.fn<NonNullable<ManagerApi['createDirectory']>>().mockResolvedValue(success({ cwd: '/created' })),
    completeDirectory: vi.fn<ManagerApi['completeDirectory']>().mockResolvedValue(success({ matches: [] })),
  };
  const config: DirectoryPromptConfig = { id, title: 'New session on host', api, submit: vi.fn(async () => {}) };
  return { api, config, close: vi.fn() };
}
async function show(f = fixture()) {
  const view = render(<DirectoryPrompt config={f.config} onClose={f.close} />);
  await act(async () => {});
  await complete();
  return { ...f, view };
}
function draft(value: string) { fireEvent.change(screen.getByRole('textbox', { name: 'Host directory' }), { target: { value } }); }
async function press(name: string) { await act(async () => { fireEvent.click(screen.getByRole('button', { name })); }); }
async function complete() { await act(async () => { await vi.advanceTimersByTimeAsync(100); }); }
beforeEach(() => { vi.clearAllMocks(); vi.useFakeTimers(); });
afterEach(() => { cleanup(); vi.useRealTimers(); });

describe('mobile directory prompt', () => {
  it('resolves an existing folder and submits once with OK or the keyboard', async () => {
    const f = await show(); draft('~/work');
    await act(async () => { fireEvent.keyDown(screen.getByRole('textbox'), { key: 'Enter' }); });
    expect(f.api.resolveDirectory).toHaveBeenCalledExactlyOnceWith({ path: '~/work' });
    expect(f.api.createDirectory).not.toHaveBeenCalled();
    expect(f.config.submit).toHaveBeenCalledExactlyOnceWith('/resolved', expect.any(AbortSignal));
    expect(f.close).toHaveBeenCalledOnce();
  });
  it('offers recursive creation only for NOT_FOUND, and No restores the unchanged draft', async () => {
    const f = fixture(); f.api.resolveDirectory.mockResolvedValue(failure('NOT_FOUND', 'Missing'));
    await show(f); draft('~/new/child '); await press('OK');
    expect(screen.getByText('Create this folder and any missing parents?')).toBeTruthy();
    expect(f.api.createDirectory).not.toHaveBeenCalled();
    expect(f.config.submit).not.toHaveBeenCalled();
    await press('No');
    expect(screen.getByRole('textbox')).toHaveProperty('value', '~/new/child ');
    await press('OK'); await press('Yes');
    expect(f.api.createDirectory).toHaveBeenCalledExactlyOnceWith({ path: '~/new/child ' });
    expect(f.config.submit).toHaveBeenCalledWith('/created', expect.any(AbortSignal));
  });
  it.each(['VALIDATION', 'ACCESS_DENIED', 'INTERNAL'] as const)('does not offer folder creation after %s', async code => {
    const f = fixture(); f.api.resolveDirectory.mockResolvedValue(failure(code, 'Cannot resolve folder.'));
    await show(f); await press('OK');
    expect(screen.getByText('Cannot resolve folder.')).toBeTruthy();
    expect(screen.queryByRole('button', { name: 'Yes' })).toBeNull();
    expect(f.api.createDirectory).not.toHaveBeenCalled();
  });
  it('keeps the confirmation and draft after failed creation and allows retry', async () => {
    const f = fixture(); f.api.resolveDirectory.mockResolvedValue(failure('NOT_FOUND', 'Missing'));
    f.api.createDirectory.mockResolvedValueOnce(failure('ACCESS_DENIED', 'Permission denied.'));
    await show(f); draft('~/new'); await press('OK'); await press('Yes');
    expect(screen.getByText('Permission denied.')).toBeTruthy(); expect(f.close).not.toHaveBeenCalled();
    await press('Yes'); expect(f.config.submit).toHaveBeenCalledOnce();
  });
  it('keeps a failed session submission editable and retries without closing', async () => {
    const f = fixture(); vi.mocked(f.config.submit).mockRejectedValueOnce(new Error('Retry session.'));
    await show(f); await press('OK');
    expect(screen.getByText('Retry session.')).toBeTruthy(); expect(f.close).not.toHaveBeenCalled();
    await press('OK'); expect(f.config.submit).toHaveBeenCalledTimes(2); expect(f.close).toHaveBeenCalledOnce();
  });
  it('shows parent matches while typing and chooses on the first touch without submitting', async () => {
    const f = fixture(); f.api.completeDirectory.mockImplementation(async ({ path }) => success({ matches: path === '~/work' ? ['~/work/', '~/workshop/'] : [] }));
    await show(f); await act(async () => { draft('~/work'); }); await complete();
    expect(screen.getByRole('button', { name: '~/work/' })).toHaveProperty('textContent', 'work');
    const suggestion = screen.getByRole('button', { name: '~/workshop/' });
    expect(suggestion.parentElement?.getAttribute('data-taps')).toBe('always');
    await press('~/workshop/');
    expect(screen.getByRole('textbox')).toHaveProperty('value', '~/workshop/');
    expect(f.api.resolveDirectory).not.toHaveBeenCalled(); expect(f.config.submit).not.toHaveBeenCalled();
    await complete(); expect(f.api.completeDirectory).toHaveBeenLastCalledWith({ path: '~/workshop/' });
  });
  it('hides a sole exact completion with a trailing separator', async () => {
    const f = fixture(); f.api.completeDirectory.mockResolvedValue(success({ matches: ['~/work/'] }));
    await show(f); await act(async () => { draft('~/work'); }); await complete();
    expect(screen.queryByRole('button', { name: '~/work/' })).toBeNull();
  });
  it('ignores out-of-order completions and clears choices as soon as the draft changes', async () => {
    const f = fixture(); const old = deferred<Result<{ matches: string[] }>>();
    f.api.completeDirectory.mockImplementation(async ({ path }) => path === '~/old' ? old.promise : success({ matches: ['~/new-a/', '~/new-b/'] }));
    await show(f); await act(async () => { draft('~/old'); }); await complete();
    await act(async () => { draft('~/new'); }); await complete();
    await act(async () => { old.resolve(success({ matches: ['~/old-a/'] })); });
    expect(screen.queryByRole('button', { name: '~/old-a/' })).toBeNull();
    expect(screen.getByRole('button', { name: '~/new-a/' })).toBeTruthy();
  });
  it('preserves literal filename spaces for resolution rather than trimming the draft', async () => {
    const f = await show(); draft('/home/user/literal space '); await press('OK');
    expect(f.api.resolveDirectory).toHaveBeenCalledExactlyOnceWith({ path: '/home/user/literal space ' });
  });
  it('debounces rapid typing into one completion request for the latest draft', async () => {
    const f = await show(); f.api.completeDirectory.mockClear();
    draft('~/w'); await act(async () => { await vi.advanceTimersByTimeAsync(50); });
    draft('~/wo'); await act(async () => { await vi.advanceTimersByTimeAsync(50); });
    draft('~/work'); await act(async () => { await vi.advanceTimersByTimeAsync(99); });
    expect(f.api.completeDirectory).not.toHaveBeenCalled();
    await act(async () => { await vi.advanceTimersByTimeAsync(1); });
    expect(f.api.completeDirectory).toHaveBeenCalledExactlyOnceWith({ path: '~/work' });
  });
  it('shows compact basenames without treating POSIX backslashes as separators', async () => {
    const f = fixture(); f.api.completeDirectory.mockResolvedValue(success({ matches: ['/home/user/name\\part/', 'C:\\home/work\\folder\\'] }));
    await show(f);
    expect(screen.getByRole('button', { name: '/home/user/name\\part/' })).toHaveProperty('textContent', 'name\\part');
    expect(screen.getByRole('button', { name: 'C:\\home/work\\folder\\' })).toHaveProperty('textContent', 'folder');
  });
  it('prevents rapid double submits and hardware-back dismissal during submission', async () => {
    const f = fixture(); const resolve = deferred<Result<{ cwd: string }>>(); f.api.resolveDirectory.mockReturnValue(resolve.promise);
    await show(f);
    await act(async () => { fireEvent.click(screen.getByRole('button', { name: 'OK' })); fireEvent.click(screen.getByRole('button', { name: 'OK' })); });
    fireEvent.keyDown(screen.getByRole('dialog'), { key: 'Escape' });
    expect(f.close).not.toHaveBeenCalled(); expect(f.api.resolveDirectory).toHaveBeenCalledOnce();
    await act(async () => { resolve.resolve(success({ cwd: '/done' })); }); expect(f.config.submit).toHaveBeenCalledOnce();
  });
  it('allows cancelling a pending home lookup and ignores its late response', async () => {
    const f = fixture(); const home = deferred<Result<{ cwd: string }>>(); f.api.getHomeDirectory.mockReturnValue(home.promise);
    const view = render(<DirectoryPrompt config={f.config} onClose={f.close} />);
    await press('Cancel'); expect(f.close).toHaveBeenCalledOnce();
    await act(async () => { home.resolve(success({ cwd: '/late' })); });
    expect(f.api.completeDirectory).not.toHaveBeenCalled(); view.unmount();
  });
  it('resets for a replacement modal and ignores an old resolution', async () => {
    const first = fixture(); const resolved = deferred<Result<{ cwd: string }>>(); first.api.resolveDirectory.mockReturnValue(resolved.promise);
    const { view } = await show(first); await press('OK');
    const next = fixture('dialog-2');
    await act(async () => { view.rerender(<DirectoryPrompt config={next.config} onClose={next.close} />); });
    await act(async () => { resolved.resolve(success({ cwd: '/stale' })); });
    expect(first.config.submit).not.toHaveBeenCalled(); expect(first.close).not.toHaveBeenCalled();
    expect(screen.getByRole('textbox')).toHaveProperty('value', '/home/user');
  });
  it('aborts an in-flight session callback on unmount and does not close the next modal', async () => {
    const f = fixture(); const saved = deferred<void>(); vi.mocked(f.config.submit).mockReturnValue(saved.promise);
    const { view } = await show(f); await press('OK');
    const signal = vi.mocked(f.config.submit).mock.calls[0][1]; view.unmount(); expect(signal.aborted).toBe(true);
    await act(async () => { saved.resolve(); }); expect(f.close).not.toHaveBeenCalled();
  });
  it('survives strict-effect replay without accepting the first home response', async () => {
    const f = fixture(); const old = deferred<Result<{ cwd: string }>>(); f.api.getHomeDirectory.mockReturnValueOnce(old.promise);
    render(<StrictMode><DirectoryPrompt config={f.config} onClose={f.close} /></StrictMode>);
    await act(async () => {}); await act(async () => { old.resolve(success({ cwd: '/old' })); });
    expect(screen.getByRole('textbox')).toHaveProperty('value', '/home/user');
  });
  it('exposes home lookup errors with a retry and blocks empty submission', async () => {
    const f = fixture(); f.api.getHomeDirectory.mockResolvedValueOnce(failure('INTERNAL', 'Home unavailable.'));
    await show(f); expect(screen.getByText('Home unavailable.')).toBeTruthy();
    expect(screen.getByRole('button', { name: 'OK' })).toHaveProperty('disabled', true);
    await press('Retry'); expect(screen.getByRole('textbox')).toHaveProperty('value', '/home/user');
  });
  it('uses the native fixture parent suggestions and recursively creates missing parents', async () => {
    const created = vi.fn(); const api = createDirectoryFixtureApi(created);
    expect(await api.getHomeDirectory()).toEqual(success({ cwd: '~/' }));
    expect(await api.completeDirectory({ path: '~/wo' })).toEqual(success({ matches: ['~/work/', '~/workshop/'] }));
    expect(await api.completeDirectory({ path: '~/work/' })).toEqual(success({ matches: ['~/work/src/'] }));
    expect(await api.resolveDirectory({ path: '~/new/child' })).toMatchObject({ ok: false, error: { code: 'NOT_FOUND' } });
    expect(await api.createDirectory({ path: '~/new/child' })).toEqual(success({ cwd: '/home/mobile/new/child' }));
    expect(await api.resolveDirectory({ path: '~/new' })).toEqual(success({ cwd: '/home/mobile/new' }));
    expect(await api.resolveDirectory({ path: '~/new/child' })).toEqual(success({ cwd: '/home/mobile/new/child' }));
    expect(created).toHaveBeenCalledExactlyOnceWith('/home/mobile/new/child');
  });
  it.each(['android', 'ios'])('uses the shared keyboard layout for %s', async os => {
    const { Platform } = await import('react-native'); Platform.OS = os as typeof Platform.OS;
    await show(); const layout = screen.getByTestId('keyboard-layout');
    expect(layout.getAttribute('data-style')).toContain('"paddingBottom":120');
    expect(layout.getAttribute('data-offset')).toBe('24');
    expect(layout.getAttribute('data-behavior')).toBe(os === 'ios' ? 'padding' : null);
  });
});
