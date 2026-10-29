// @vitest-environment jsdom
import '@testing-library/jest-dom/vitest';
import './terminal-test-mocks';
import { terminalMocks } from './terminal-test-mocks';
import { afterEach, expect, it, vi } from 'vitest';
import { act, cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { success, type OwnedTerminalAttachmentDto } from '../shared/contracts';
import { App } from './App';
import { createManagerClient } from './store';
import { TerminalRegistry, createTerminalSurface } from './terminal-client';
import { mockApi, session, snapshot } from './test-fixtures';

const flush = async () => { for (let i = 0; i < 30; i++) await Promise.resolve(); };
afterEach(cleanup);
function automation(closed = true) {
  const item = session(); item.canAddTab = false;
  const tab = item.tabs[0]; tab.profileId = 'automation-pipe'; tab.title = 'Build';
  tab.lifecycle = closed ? 'closed' : 'open'; tab.exitCode = closed ? 7 : null;
  const replay: OwnedTerminalAttachmentDto = {
    control: 'owned', controlEpoch: 1, tabId: tab.id, sessionId: item.id, generation: tab.generation,
    firstSequence: 1, lastSequence: 1, chunks: [{ type: 'data', tabId: tab.id, generation: tab.generation, sequence: 1, data: 'retained build output' }],
    truncated: false, state: closed ? 'closed' : 'open', exitCode: tab.exitCode,
    cols: 80, rows: 24, lifetime: 'app-owned',
  };
  const f = mockApi(snapshot([item])); f.api.attachTerminal.mockResolvedValue(success(replay));
  return { ...f, item, tab };
}

it('restores and selects a completed automation tab, clearing busy without opening a replacement shell', async () => {
  const f = automation(), client = createManagerClient(f.api, { selectedId: f.item.id, activeTabIds: { [f.item.id]: f.tab.id } });
  client.start(); await flush();
  try {
    client.selectTab(f.item, f.tab.id);
    expect(client.store.getState()).toMatchObject({ selectedId: f.item.id, activeTabIds: { [f.item.id]: f.tab.id } });
    f.emitTerminal({ type: 'activity', tabId: f.tab.id, busy: true });
    f.emit({ ...snapshot([f.item]), revision: 2 }); await flush();
    expect(client.store.getState().activeTabIds[f.item.id]).toBe(f.tab.id);
    expect(client.store.getState().busyTabIds[f.tab.id]).toBeUndefined();
    expect(f.api.activateSession).not.toHaveBeenCalled(); expect(f.api.addTab).not.toHaveBeenCalled();
  } finally { client.stop(); }
});

it.each([true, false])('renders automation replay read-only when closed=%s, including sidebar and settings round trips', async closed => {
  const f = automation(closed);
  render(<App api={f.api} />);
  expect(await screen.findByRole('tab', { name: /Build/ })).toHaveAttribute('aria-selected', 'true');
  await waitFor(() => expect(screen.getByLabelText('Read-only terminal')).toHaveTextContent('retained build output'));
  expect(screen.getByRole('button', { name: 'New terminal' })).toBeDisabled();
  const textarea = screen.getByRole('textbox', { name: 'Terminal input' });
  expect(textarea).toHaveAttribute('readonly');
  fireEvent.input(textarea, { target: { value: 'must not reach stdin' } });
  const terminal = terminalMocks.terminals.at(-1)!;
  terminal.input('encoded key'); terminal.input('\x1b[1;1R');
  await act(flush);
  expect(f.api.writeTerminal).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole('button', { name: 'Settings' }));
  fireEvent.click(screen.getByRole('button', { name: 'Settings' }));
  fireEvent.click(screen.getByRole('button', { name: `Select session ${f.item.title}` }));
  await waitFor(() => expect(screen.getByRole('tab', { name: /Build/ })).toHaveAttribute('aria-selected', 'true'));
  expect(screen.getByLabelText('Read-only terminal')).toHaveTextContent('retained build output');
  expect(f.api.addTab).not.toHaveBeenCalled(); expect(f.api.createSession).not.toHaveBeenCalled();
  expect(f.item.tabs[0].lifecycle).toBe(closed ? 'closed' : 'open');
});

it('keeps read-only registry input fenced across visibility changes and replay refreshes', async () => {
  const f = automation(false), registry = new TerminalRegistry(f.api), host = document.createElement('div');
  document.body.append(host); registry.start();
  const unmount = registry.mount(f.tab.id, host, true, true);
  try {
    await flush();
    registry.visibility(f.tab.id, false); registry.visibility(f.tab.id, true); registry.refresh(f.tab.id);
    await flush();
    const terminal = terminalMocks.terminals.at(-1)!;
    expect(terminal.options.disableStdin).toBe(true); expect(terminal.textarea.readOnly).toBe(true);
    terminal.input('typed'); terminal.input('query reply'); await flush();
    expect(f.api.writeTerminal).not.toHaveBeenCalled();
    expect(registry.getState(f.tab.id).operationError).toBeNull();
  } finally { unmount(); registry.dispose(); host.remove(); }
});

it('allows selection copy but suppresses clipboard reads, paste and parser replies when input is disabled', async () => {
  const input = vi.fn(), copy = vi.fn(), read = vi.fn(async () => 'paste');
  const surface = createTerminalSurface(input, undefined, { copy, read });
  document.body.append(surface.element); surface.fit(); surface.setInput(false);
  try {
    const terminal = terminalMocks.terminals.at(-1)!;
    terminal.selection = 'selected output';
    const shortcut = (code: string) => new KeyboardEvent('keydown', { code, ctrlKey: true, shiftKey: true });
    terminal.keyHandler(shortcut('KeyC')); terminal.keyHandler(shortcut('KeyV'));
    terminal.input('device reply'); await flush();
    expect(copy).toHaveBeenCalledExactlyOnceWith('selected output');
    expect(read).not.toHaveBeenCalled(); expect(terminal.paste).not.toHaveBeenCalled(); expect(input).not.toHaveBeenCalled();
  } finally { surface.dispose(); }
});
