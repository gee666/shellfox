// @vitest-environment jsdom
import React, { StrictMode, type ReactNode, type Ref } from 'react';
import { act, cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { ManagerApi, Result } from '../../src/shared/contracts';
import { failure, success } from '../../src/shared/contracts';
import { DirectoryPrompt, type DirectoryPromptConfig } from './DirectoryPrompt';
import { DirectoryPathInput, type DirectoryPathInputHandle } from './DirectoryPathInput';
import { createDirectoryFixtureApi } from '../e2e/DirectoryHarness';

type NativeProps = {
  children?: ReactNode; accessibilityLabel?: string; accessibilityRole?: string; disabled?: boolean;
  onPress?: () => void; onRequestClose?: () => void; keyboardShouldPersistTaps?: string;
  behavior?: string; keyboardVerticalOffset?: number; style?: unknown;
};
type NativeInputProps = {
  ref?: Ref<HTMLInputElement>; value: string; onChangeText: (value: string) => void;
  onSubmitEditing: (event: { nativeEvent: KeyboardEvent }) => void;
  onKeyPress?: (event: { nativeEvent: KeyboardEvent; stopPropagation(): void }) => void;
  onFocus?: () => void; onPressIn?: () => void; editable?: boolean; accessibilityLabel: string; style?: unknown; disableFullscreenUI?: boolean;
};
const keyboard = vi.hoisted(() => ({ keyboardVisible: true, keyboardInset: 120, keyboardOffset: 24, measureKeyboardInset: vi.fn() }));
vi.mock('react-native-safe-area-context', () => ({ useSafeAreaInsets: () => ({ top: 30, bottom: 20 }) }));
vi.mock('./terminal/useKeyboardOverlap', () => ({ useKeyboardOverlap: () => keyboard }));
vi.mock('react-native', () => ({
  Platform: { OS: 'android' }, StyleSheet: { create: <T,>(styles: T) => styles },
  View: ({ children, style }: NativeProps) => <div data-style={JSON.stringify(style)}>{children}</div>,
  Text: ({ children, accessibilityLabel }: NativeProps) => <span aria-label={accessibilityLabel}>{children}</span>,
  Pressable: ({ children, accessibilityLabel, disabled, onPress }: NativeProps) => <button aria-label={accessibilityLabel} disabled={disabled} onClick={onPress}>{children}</button>,
  ScrollView: ({ children, keyboardShouldPersistTaps }: NativeProps) => <div data-taps={keyboardShouldPersistTaps}>{children}</div>,
  Modal: ({ children, onRequestClose }: NativeProps) => <div role="dialog" onKeyDown={event => { if (event.key === 'Escape') onRequestClose?.(); }}>{children}</div>,
  KeyboardAvoidingView: ({ children, behavior, keyboardVerticalOffset, style }: NativeProps) => <div data-testid="keyboard-layout" data-behavior={behavior} data-offset={keyboardVerticalOffset} data-style={JSON.stringify(style)}>{children}</div>,
  TextInput: ({ ref, value, onChangeText, onSubmitEditing, onKeyPress, onFocus, onPressIn, editable, accessibilityLabel, style, disableFullscreenUI }: NativeInputProps) => (
    <input ref={ref} data-style={JSON.stringify(style)} data-extract-disabled={disableFullscreenUI} aria-label={accessibilityLabel} value={value} disabled={editable === false}
      onFocus={onFocus} onMouseDown={onPressIn} onChange={event => onChangeText(event.target.value)}
      onKeyDown={event => {
        onKeyPress?.({ nativeEvent: event.nativeEvent, stopPropagation: () => event.stopPropagation() });
        if (event.key === 'Enter') onSubmitEditing({ nativeEvent: event.nativeEvent });
      }} />
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

describe('reusable mobile directory path input', () => {
  it('uses a compact caption-free 36dp field without a hidden home lookup in the file path sheet', async () => {
    const { api } = fixture();
    render(<DirectoryPathInput api={api} value="/remote/work" label="File path" compact onChangeText={vi.fn()} onSubmit={vi.fn()} />);
    await act(async () => {}); await complete();
    const input = screen.getByRole('textbox', { name: 'File path' });
    expect(screen.queryByText('File path')).toBeNull(); expect(screen.queryByLabelText(/Home directory/)).toBeNull();
    expect(input.getAttribute('data-style')).toContain('"minHeight":34');
    expect(input.getAttribute('data-extract-disabled')).toBe('true');
    expect(input.parentElement?.getAttribute('data-style')).toContain('"minHeight":36');
    expect(api.getHomeDirectory).not.toHaveBeenCalled();
  });
  it('loads the remote home indicator without overwriting the controlled draft', async () => {
    const { api } = fixture(); const submit = vi.fn(); const close = vi.fn();
    const handle = React.createRef<DirectoryPathInputHandle>();
    function Entry() {
      const [value, setValue] = React.useState('');
      return <DirectoryPathInput ref={handle} api={api} value={value} onChangeText={setValue}
        onSubmit={submit} onClose={close} />;
    }
    const view = render(<Entry />); await act(async () => {}); await complete();
    expect(api.getHomeDirectory).toHaveBeenCalledOnce();
    expect(screen.getByLabelText('Home directory: /home/user')).toBeTruthy();
    expect(screen.getByRole('textbox')).toHaveProperty('value', '');
    expect(screen.getByRole('textbox').getAttribute('data-extract-disabled')).toBe('false');
    await act(async () => { fireEvent.keyDown(screen.getByRole('textbox'), { key: 'Enter' }); });
    expect(submit).toHaveBeenCalledExactlyOnceWith('');
    expect(handle.current?.dismissSuggestions()).toBe(false);
    fireEvent.keyDown(screen.getByRole('textbox'), { key: 'Escape' });
    expect(close).toHaveBeenCalledOnce(); view.unmount();
  });
  it('shares an existing home lookup and stops stale completions after unmount', async () => {
    const { api } = fixture(); const completion = deferred<Result<{ matches: string[] }>>();
    api.completeDirectory.mockReturnValue(completion.promise);
    const view = render(<DirectoryPathInput api={api} homeDirectory="/remote/home" value="~/w"
      onChangeText={vi.fn()} onSubmit={vi.fn()} />);
    expect(screen.getByLabelText('Home directory: /remote/home')).toBeTruthy();
    await complete(); view.unmount();
    await act(async () => { completion.resolve(success({ matches: ['~/work/', '~/workshop/'] })); });
    expect(api.getHomeDirectory).not.toHaveBeenCalled();
  });
});

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
  it.each(['', '~', '~/'])('dismisses suggestions before submitting the exact home draft %j', async path => {
    const f = fixture();
    f.api.completeDirectory.mockResolvedValue(success({ matches: ['~/work/', '~/workshop/'] }));
    await show(f); draft(path); await complete();
    const input = screen.getByRole('textbox');
    fireEvent.keyDown(input, { key: 'Escape' });
    expect(screen.queryByRole('button', { name: '~/work/' })).toBeNull();
    expect(f.close).not.toHaveBeenCalled();
    await complete();
    expect(screen.queryByRole('button', { name: '~/work/' })).toBeNull();
    await act(async () => { fireEvent.keyDown(input, { key: 'Enter' }); });
    expect(f.api.resolveDirectory).toHaveBeenCalledExactlyOnceWith({ path });
  });
  it('selects the first suggestion with Enter instead of resolving the current home', async () => {
    const f = fixture(); f.api.completeDirectory.mockResolvedValue(success({ matches: ['~/work/', '~/workshop/'] }));
    await show(f);
    fireEvent.keyDown(screen.getByRole('textbox'), { key: 'Enter' });
    expect(screen.getByRole('textbox')).toHaveProperty('value', '~/work/');
    expect(f.api.resolveDirectory).not.toHaveBeenCalled();
  });
  it('consumes Android Back or hardware Escape from the modal before closing', async () => {
    const f = fixture(); f.api.completeDirectory.mockResolvedValue(success({ matches: ['~/work/', '~/workshop/'] }));
    await show(f);
    fireEvent.keyDown(screen.getByRole('dialog'), { key: 'Escape' });
    expect(screen.queryByRole('button', { name: '~/work/' })).toBeNull();
    expect(f.close).not.toHaveBeenCalled();
    fireEvent.keyDown(screen.getByRole('dialog'), { key: 'Escape' });
    expect(f.close).toHaveBeenCalledOnce();
  });
  it('closes on a second input Escape without bubbling the first into modal close', async () => {
    const f = fixture(); f.api.completeDirectory.mockResolvedValue(success({ matches: ['~/work/', '~/workshop/'] }));
    await show(f); const input = screen.getByRole('textbox');
    fireEvent.keyDown(input, { key: 'Escape' }); expect(f.close).not.toHaveBeenCalled();
    fireEvent.keyDown(input, { key: 'Escape' }); expect(f.close).toHaveBeenCalledOnce();
  });
  it.each(['press', 'focus', 'type'])('reopens a dismissed menu on %s', async interaction => {
    const f = fixture(); f.api.completeDirectory.mockResolvedValue(success({ matches: ['~/work/', '~/workshop/'] }));
    await show(f); const input = screen.getByRole('textbox');
    fireEvent.keyDown(input, { key: 'Escape' });
    if (interaction === 'press') fireEvent.mouseDown(input);
    else if (interaction === 'focus') fireEvent.focus(input);
    else draft('~/w');
    await complete();
    expect(screen.getByRole('button', { name: '~/work/' })).toBeTruthy();
  });
  it.each([{ isComposing: true }, { keyCode: 229 }])('leaves composition Enter/Escape to the IME: %j', async metadata => {
    const f = fixture(); f.api.completeDirectory.mockResolvedValue(success({ matches: ['~/work/', '~/workshop/'] }));
    await show(f); const input = screen.getByRole('textbox');
    fireEvent.keyDown(input, { key: 'Enter', ...metadata });
    fireEvent.keyDown(input, { key: 'Escape', ...metadata });
    expect(input).toHaveProperty('value', '/home/user');
    expect(screen.getByRole('button', { name: '~/work/' })).toBeTruthy();
    expect(f.api.resolveDirectory).not.toHaveBeenCalled(); expect(f.close).not.toHaveBeenCalled();
    fireEvent.keyDown(input, { key: 'Escape' });
    fireEvent.keyDown(input, { key: 'Enter', ...metadata });
    expect(f.api.resolveDirectory).not.toHaveBeenCalled();
    await act(async () => { fireEvent.keyDown(input, { key: 'Enter' }); });
    expect(f.api.resolveDirectory).toHaveBeenCalledExactlyOnceWith({ path: '/home/user' });
  });
  it('does not resurrect a dismissed menu when an older completion resolves', async () => {
    const f = fixture(); const next = deferred<Result<{ matches: string[] }>>();
    f.api.completeDirectory.mockResolvedValue(success({ matches: ['~/work/', '~/workshop/'] }));
    await show(f);
    fireEvent.keyDown(screen.getByRole('textbox'), { key: 'Escape' });
    f.api.completeDirectory.mockReturnValue(next.promise);
    draft('~/next'); await complete();
    fireEvent.keyDown(screen.getByRole('textbox'), { key: 'Escape' });
    await act(async () => { next.resolve(success({ matches: ['~/next-a/', '~/next-b/'] })); });
    expect(screen.queryByRole('button', { name: '~/next-a/' })).toBeNull();
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
