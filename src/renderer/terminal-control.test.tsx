// @vitest-environment jsdom
import '@testing-library/jest-dom/vitest';
import './terminal-test-mocks';
import { act, cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, expect, it, vi } from 'vitest';
import type { BlockedTerminalAttachmentDto, OwnedTerminalAttachmentDto, Result, TerminalAttachmentDto, TerminalEvent } from '../shared/contracts';
import { failure, success } from '../shared/contracts';
import { TerminalRegistry, type SurfaceFactory } from './terminal-client';
import { TerminalViewport } from './TerminalViewport';
import { deferred, mockApi, session } from './test-fixtures';

const tab = session().tabs[0]!, identity = { tabId: tab.id, generation: tab.generation };
const owned = (controlEpoch = 1, data = ''): OwnedTerminalAttachmentDto => ({
  ...identity, sessionId: tab.sessionId, control: 'owned', controlEpoch, cols: 115, rows: 45,
  firstSequence: 1, lastSequence: data ? 1 : 0, chunks: data ? [{ type: 'data', ...identity, sequence: 1, data }] : [],
  truncated: false, state: 'open', exitCode: null, lifetime: 'app-owned',
});
const blocked = (controlEpoch = 2): BlockedTerminalAttachmentDto => ({ ...identity, sessionId: tab.sessionId, control: 'blocked', controlEpoch });
const control = (state: 'owned' | 'blocked', controlEpoch: number): TerminalEvent => ({ type: 'control', ...identity, control: state, controlEpoch });
const flush = async () => { for (let i = 0; i < 40; i++) await Promise.resolve(); };
const registries: TerminalRegistry[] = [];
afterEach(() => { cleanup(); registries.splice(0).forEach(registry => registry.dispose()); document.body.replaceChildren(); });
function setup(initial: Result<TerminalAttachmentDto> = success(owned()), delayed = false, viewport = false) {
  const f = mockApi(); f.api.attachTerminal.mockResolvedValue(initial);
  const screens: { output: string; callbacks: (() => void)[]; input: (data: string, userInitiated?: boolean) => void; dispose: ReturnType<typeof vi.fn>; fit: ReturnType<typeof vi.fn> }[] = [];
  const factory: SurfaceFactory = input => {
    const item = { output: '', callbacks: [] as (() => void)[], input, dispose: vi.fn(), fit: vi.fn(() => ({ cols: 100, rows: 40 })) };
    screens.push(item);
    const element = document.createElement('div');
    return { element, write(data, done) { item.output += data; element.textContent = item.output; if (delayed) item.callbacks.push(done); else done(); }, reset() {}, resize() {}, fit: item.fit, focus() {}, setInput() {}, dispose() { item.dispose(); element.remove(); } };
  };
  const registry = new TerminalRegistry(f.api, factory); registries.push(registry); registry.start();
  let unmount: () => void;
  if (viewport) unmount = render(<TerminalViewport registry={registry} tabId={tab.id} visible />).unmount;
  else { const host = document.createElement('div'); document.body.append(host); unmount = registry.mount(tab.id, host, true); }
  return { ...f, registry, screens, unmount, latest: () => screens.at(-1)! };
}

it('shows a blank blocked terminal with Take it and never fits, writes, or acknowledges it', async () => {
  const f = setup(success(blocked()), false, true); await act(flush);
  expect(screen.getByText('Another client is controlling this terminal')).toBeVisible();
  expect(screen.getByRole('button', { name: 'Take it' })).toBeEnabled();
  expect(screen.getByLabelText('Interactive terminal')).not.toBeVisible();
  f.latest().input('hidden'); f.registry.fit(tab.id); await flush();
  expect(f.latest().fit).not.toHaveBeenCalled(); expect(f.api.resizeTerminal).not.toHaveBeenCalled();
  expect(f.api.writeTerminal).not.toHaveBeenCalled(); expect(f.api.acknowledgeTerminal).not.toHaveBeenCalled();
});

it('discards old content/parser callbacks on loss, then takes control through a fresh full attach', async () => {
  const f = setup(success(owned(1, 'private old screen')), true, true); await act(flush);
  const old = f.latest();
  await act(async () => { f.emitTerminal(control('blocked', 2)); });
  expect(old.dispose).toHaveBeenCalledOnce(); expect(screen.queryByText('private old screen')).not.toBeInTheDocument();
  old.callbacks.forEach(done => done()); old.input('old input');
  f.emitTerminal({ type: 'data', ...identity, controlEpoch: 1, sequence: 2, data: 'stale' }); await flush();
  expect(f.api.acknowledgeTerminal).not.toHaveBeenCalled(); expect(f.api.writeTerminal).not.toHaveBeenCalled();
  expect(f.api.detachTerminal).not.toHaveBeenCalled();
  f.api.attachTerminal.mockResolvedValue(success(owned(3, 'fresh screen')));
  const take = deferred<Result<{ taken: true }>>(); f.api.takeTerminalControl.mockReturnValueOnce(take.promise);
  fireEvent.click(screen.getByRole('button', { name: 'Take it' })); await act(flush);
  expect(f.api.takeTerminalControl).toHaveBeenCalledExactlyOnceWith(identity);
  expect(screen.getByRole('button', { name: 'Take it' })).toBeDisabled();
  await act(async () => { f.emitTerminal(control('owned', 3)); take.resolve(success({ taken: true })); await flush(); });
  expect(f.api.attachTerminal).toHaveBeenLastCalledWith({ tabId: tab.id });
  expect(f.latest().output).toBe('fresh screen');
  f.latest().callbacks.forEach(done => done()); await act(flush);
  expect(f.registry.getState(tab.id)).toMatchObject({ phase: 'open', controlEpoch: 3 });
  expect(f.api.resizeTerminal).toHaveBeenLastCalledWith({ ...identity, controlEpoch: 3, cols: 100, rows: 40 });
  expect(f.api.acknowledgeTerminal).toHaveBeenLastCalledWith({ ...identity, controlEpoch: 3, sequence: 1 });
  expect(f.api.detachTerminal).not.toHaveBeenCalled();
});

it('does not grant stream or input on an owned event until its fresh attachment succeeds', async () => {
  const f = setup(success(blocked())); await flush();
  const pending = deferred<Result<TerminalAttachmentDto>>(); f.api.attachTerminal.mockReturnValueOnce(pending.promise);
  f.emitTerminal(control('owned', 3)); await flush();
  f.latest().input('not granted yet');
  f.emitTerminal({ type: 'data', ...identity, controlEpoch: 3, sequence: 1, data: 'not a snapshot' });
  expect(f.latest().output).toBe(''); expect(f.api.writeTerminal).not.toHaveBeenCalled();
  pending.resolve(success(owned(3, 'snapshot'))); await flush();
  expect(f.latest().output).toBe('snapshot');
});

it('drops queued old-owner input even if ownership returns before the queued operation runs', async () => {
  const f = setup(); await flush();
  const pending = deferred<Result<{ written: true }>>(); f.api.writeTerminal.mockReturnValueOnce(pending.promise);
  f.latest().input('first'); f.latest().input('must not cross ownership'); await flush();
  f.emitTerminal(control('blocked', 2));
  f.api.attachTerminal.mockResolvedValue(success(owned(3)));
  f.emitTerminal(control('owned', 3)); await flush();
  f.latest().input('new owner'); pending.resolve(success({ written: true })); await flush();
  expect(f.api.writeTerminal.mock.calls.map(([input]) => [input.data, input.controlEpoch])).toEqual([['first', 1], ['new owner', 3]]);
});

it('fences an old ACK completion before granting fresh delivery credit', async () => {
  const f = setup(success(owned(1, 'old')));
  const pending = deferred<Result<{ acknowledged: true }>>(); f.api.acknowledgeTerminal.mockReturnValueOnce(pending.promise);
  await flush();
  f.emitTerminal(control('blocked', 2)); f.api.attachTerminal.mockResolvedValue(success(owned(3, 'new')));
  f.emitTerminal(control('owned', 3)); await flush();
  expect(f.api.attachTerminal).toHaveBeenCalledTimes(1);
  pending.resolve(failure('TERMINAL_CONTROL_REQUIRED', 'Old owner')); await flush();
  expect(f.api.acknowledgeTerminal.mock.calls.map(([input]) => input.controlEpoch)).toEqual([1, 3]);
  expect(f.registry.getState(tab.id)).toMatchObject({ phase: 'open', controlEpoch: 3, error: null });
  expect(f.api.detachTerminal).not.toHaveBeenCalled();
});

it('does not send an old queued fit after control is lost and regained', async () => {
  const f = setup(), pending = deferred<Result<{ resized: true }>>();
  f.api.resizeTerminal.mockReturnValueOnce(pending.promise); await flush();
  f.latest().fit.mockReturnValue({ cols: 105, rows: 41 }); f.registry.fit(tab.id);
  f.emitTerminal(control('blocked', 2)); f.api.attachTerminal.mockResolvedValue(success(owned(3)));
  f.emitTerminal(control('owned', 3)); await flush();
  pending.resolve(success({ resized: true })); await flush();
  expect(f.api.resizeTerminal.mock.calls.map(([input]) => [input.controlEpoch, input.cols])).toEqual([[1, 100], [3, 100]]);
});

it('ignores an old successful attach response after a newer blocked control event', async () => {
  const f = setup(); await flush();
  const pending = deferred<Result<TerminalAttachmentDto>>(); f.api.attachTerminal.mockReturnValueOnce(pending.promise);
  f.registry.refresh(tab.id); await flush();
  f.emitTerminal(control('blocked', 2));
  pending.resolve(success(owned(1, 'stale contents'))); await flush();
  expect(f.registry.getState(tab.id)).toMatchObject({ phase: 'blocked', controlEpoch: 2 });
  expect(f.latest().output).toBe(''); expect(f.api.detachTerminal).not.toHaveBeenCalled();
});

it('handles promotion arriving before the initial blocked attachment response', async () => {
  const f = setup(success(blocked(1))); const pending = deferred<Result<TerminalAttachmentDto>>();
  f.api.attachTerminal.mockReturnValueOnce(pending.promise).mockResolvedValue(success(owned(2, 'promoted')));
  await flush();
  f.emitTerminal(control('owned', 2)); pending.resolve(success(blocked(1))); await flush();
  expect(f.registry.getState(tab.id)).toMatchObject({ phase: 'open', controlEpoch: 2 });
  expect(f.latest().output).toBe('promoted');
});

it('does not renew ownership or replay content for duplicate/stale control notifications', async () => {
  const f = setup(success(owned(3))); await flush(); f.api.attachTerminal.mockClear();
  f.emitTerminal(control('owned', 3)); f.emitTerminal(control('blocked', 2));
  f.emitTerminal({ type: 'data', ...identity, controlEpoch: 1, sequence: 1, data: 'old' }); await flush();
  expect(f.api.attachTerminal).not.toHaveBeenCalled(); expect(f.latest().output).toBe('');
  expect(f.registry.getState(tab.id)).toMatchObject({ phase: 'open', controlEpoch: 3 });
});

it('keeps Take failures blocked and detaches normally instead of closing the shell', async () => {
  const f = setup(success(blocked())); await flush();
  f.api.takeTerminalControl.mockResolvedValueOnce(failure('NATIVE_UNAVAILABLE', 'Host disconnected.'));
  await f.registry.take(tab.id);
  expect(f.registry.getState(tab.id)).toMatchObject({ phase: 'blocked', taking: false, operationError: { message: 'Host disconnected.' } });
  f.unmount(); await flush();
  expect(f.api.detachTerminal).toHaveBeenCalledWith(identity); expect(f.api.closeTab).not.toHaveBeenCalled();
});
