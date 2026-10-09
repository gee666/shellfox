// @vitest-environment jsdom
import '@testing-library/jest-dom/vitest';
import './terminal-test-mocks';
import { Profiler } from 'react';
import { useStore } from 'zustand';
import { act, cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, expect, it, vi } from 'vitest';
import { createManagerClient } from './store';
import { TerminalRegistry } from './terminal-client';
import { TerminalWorkspace } from './TerminalWorkspace';
import { TerminalViewport } from './TerminalViewport';
import { mockApi, profiles, session, snapshot } from './test-fixtures';

const registries: TerminalRegistry[] = [];
const flush = async () => { for (let i = 0; i < 80; i++) await Promise.resolve(); };
afterEach(() => { cleanup(); registries.splice(0).forEach(registry => registry.dispose()); vi.restoreAllMocks(); });

it('mounts only the selected viewport among 64 tabs and ignores unrelated activity', async () => {
  const item = session();
  item.tabs = Array.from({ length: 64 }, (_, i) => ({ ...session(i + 1).tabs[0], sessionId: item.id, title: `Shell ${i + 1}`, ordinal: i + 1 }));
  const f = mockApi(snapshot([item])); const client = createManagerClient(f.api);
  client.store.setState({ selectedId: item.id });
  const registry = new TerminalRegistry(f.api); registries.push(registry); registry.start();
  const commits = vi.fn();
  render(<Profiler id="workspace" onRender={commits}><TerminalWorkspace session={item} client={client} registry={registry} profiles={profiles} defaultProfileId="pwsh" available /></Profiler>);
  await act(flush);
  expect(screen.getAllByRole('tab')).toHaveLength(64);
  expect(screen.getAllByLabelText('Interactive terminal')).toHaveLength(1);
  expect(f.api.attachTerminal).toHaveBeenCalledTimes(1);
  commits.mockClear();
  act(() => client.store.setState({ busyTabIds: { [session(99).tabs[0].id]: true } }));
  expect(commits).not.toHaveBeenCalled();
  act(() => client.store.setState(state => ({ busyTabIds: { ...state.busyTabIds, [item.tabs[0].id]: true } })));
  expect(commits).toHaveBeenCalled();
  await act(async () => { fireEvent.click(screen.getByRole('tab', { name: /Shell 64/ })); await flush(); });
  expect(screen.getAllByLabelText('Interactive terminal')).toHaveLength(1);
  expect(f.api.attachTerminal).toHaveBeenCalledTimes(2);
  expect(f.api.detachTerminal).toHaveBeenCalledWith({ tabId: item.tabs[0].id, generation: item.tabs[0].generation });
  expect(f.api.createSession).not.toHaveBeenCalled(); expect(f.api.activateSession).not.toHaveBeenCalled(); expect(f.api.closeTab).not.toHaveBeenCalled();
});

it('skips workspace work when a broad store subscriber rerenders its parent', async () => {
  const item = session(); const f = mockApi(); const client = createManagerClient(f.api);
  const registry = new TerminalRegistry(f.api); registries.push(registry); registry.start();
  const shells = [...profiles]; const availableShells = vi.spyOn(shells, 'some'); const parentRender = vi.fn();
  function Parent() {
    useStore(client.store); parentRender();
    return <TerminalWorkspace session={item} client={client} registry={registry} profiles={shells} defaultProfileId="pwsh" available />;
  }
  render(<Parent />); await act(flush);
  parentRender.mockClear(); availableShells.mockClear();
  act(() => client.store.setState({ busyTabIds: { [session(99).tabs[0].id]: true } }));
  expect(parentRender).toHaveBeenCalledOnce(); expect(availableShells).not.toHaveBeenCalled();
  act(() => client.store.setState({ busyTabIds: { [item.tabs[0].id]: true } }));
  expect(availableShells).toHaveBeenCalledOnce();
});

it('coalesces viewport resize notifications into one frame and cancels work when hidden', async () => {
  const f = mockApi(); const registry = new TerminalRegistry(f.api); registries.push(registry); registry.start();
  let resized = () => {};
  const disconnect = vi.fn();
  vi.spyOn(globalThis, 'ResizeObserver').mockImplementation(class {
    constructor(callback: ResizeObserverCallback) { resized = () => callback([], this); }
    observe = vi.fn(); unobserve = vi.fn(); disconnect = disconnect;
  });
  const frames: FrameRequestCallback[] = [];
  const requestFrame = vi.spyOn(globalThis, 'requestAnimationFrame').mockImplementation(callback => { frames.push(callback); return frames.length - 1; });
  const cancelFrame = vi.spyOn(globalThis, 'cancelAnimationFrame').mockImplementation(() => {});
  const fit = vi.spyOn(registry, 'fit'); const refresh = vi.spyOn(registry, 'refresh');
  const tab = session().tabs[0];
  const view = render(<TerminalViewport registry={registry} tabId={tab.id} generation={tab.generation} visible />);
  await act(flush);
  expect(refresh).not.toHaveBeenCalled();
  fit.mockClear();
  for (let i = 0; i < 100; i++) resized();
  expect(requestFrame).toHaveBeenCalledOnce();
  act(() => frames[0](0)); expect(fit).toHaveBeenCalledOnce();
  resized(); resized(); expect(requestFrame).toHaveBeenCalledTimes(2);
  view.rerender(<TerminalViewport registry={registry} tabId={tab.id} generation={tab.generation} visible={false} />);
  expect(cancelFrame).toHaveBeenLastCalledWith(1); expect(disconnect).toHaveBeenCalledOnce();
  expect(f.api.closeTab).not.toHaveBeenCalled();
});
