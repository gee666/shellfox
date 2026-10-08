// @vitest-environment jsdom
import '@testing-library/jest-dom/vitest';
import './terminal-test-mocks';
import { afterEach, expect, it } from 'vitest';
import { act, cleanup, render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { App } from './App';
import { failure, success } from '../shared/contracts';
import type { ManagerApi, TerminalApi } from '../shared/contracts';
import { deferred, mockApi, session, snapshot } from './test-fixtures';

afterEach(cleanup);
const loading = () => screen.queryByRole('status', { name: 'Opening terminal' });

it('shows the quiet ASCII brand only when no terminal is open', async () => {
  const f = mockApi(snapshot([]));
  render(<App api={f.api} />);
  expect(await screen.findByText('>. Shellfox')).toBeVisible();
  const art = document.querySelector('.terminal-brand pre')!;
  expect(art.textContent).toContain('@@@');
  expect(art.textContent).not.toContain('>_');
  expect(art.textContent).toMatch(/^[\x20-\x7e\n]+$/);
  expect(art).toHaveAttribute('aria-hidden', 'true');
  expect(loading()).not.toBeInTheDocument();
  act(() => f.emit({ ...snapshot([session()]), revision: 2 }));
  await screen.findByRole('tab');
  expect(screen.queryByText('>. Shellfox')).not.toBeInTheDocument();
});

it('shows dots immediately during session activation and clears them on failure', async () => {
  const a = session(); a.tabs[0].lifecycle = 'closed';
  const f = mockApi(snapshot([a])), pending = deferred<Awaited<ReturnType<ManagerApi['activateSession']>>>();
  f.api.activateSession.mockReturnValueOnce(pending.promise);
  render(<App api={f.api} />);
  const button = await screen.findByRole('button', { name: 'Select session Session 1' });
  const user = userEvent.setup();
  await user.click(button);
  expect(loading()).toBeVisible();
  expect(screen.queryByText('>. Shellfox')).not.toBeInTheDocument();
  await user.click(button);
  expect(f.api.activateSession).toHaveBeenCalledTimes(1);
  await act(async () => pending.resolve(failure('LAUNCH_FAILED', 'Choose an available shell.')));
  expect(loading()).not.toBeInTheDocument();
  expect(screen.getByText('>. Shellfox')).toBeVisible();
  expect(screen.getByRole('alert')).toHaveTextContent('Choose an available shell.');
});

it('keeps activation indicators scoped to the selected session during rapid switching', async () => {
  const a = session(), b = session(2); a.tabs[0].lifecycle = 'closed'; b.tabs[0].lifecycle = 'closed';
  const f = mockApi(snapshot([a, b]));
  const first = deferred<Awaited<ReturnType<ManagerApi['activateSession']>>>();
  const second = deferred<Awaited<ReturnType<ManagerApi['activateSession']>>>();
  f.api.activateSession.mockReturnValueOnce(first.promise).mockReturnValueOnce(second.promise);
  render(<App api={f.api} />); const user = userEvent.setup();
  await user.click(await screen.findByRole('button', { name: 'Select session Session 1' }));
  await user.click(screen.getByRole('button', { name: 'Select session Session 2' }));
  expect(f.api.activateSession).toHaveBeenCalledTimes(2);
  await act(async () => first.resolve(success(session())));
  expect(loading()).toBeVisible();
  expect(screen.getByRole('button', { name: 'Select session Session 2' })).toHaveAttribute('aria-pressed', 'true');
  await act(async () => second.resolve(success(b)));
  expect(loading()).not.toBeInTheDocument();
});

it('shows dots while attaching and removes them when the terminal is ready', async () => {
  const f = mockApi();
  const attach = f.api.attachTerminal.getMockImplementation()!;
  const pending = deferred<Awaited<ReturnType<TerminalApi['attachTerminal']>>>();
  f.api.attachTerminal.mockReturnValueOnce(pending.promise);
  render(<App api={f.api} />);
  await waitFor(() => expect(f.api.attachTerminal).toHaveBeenCalled());
  expect(loading()).toBeVisible();
  await act(async () => pending.resolve(await attach({ tabId: session().tabs[0].id })));
  expect(loading()).not.toBeInTheDocument();
});

it('waits for shell launch before trying to attach', async () => {
  const a = session(); a.tabs[0].lifecycle = 'launching';
  const f = mockApi(snapshot([a])); render(<App api={f.api} />);
  await screen.findByRole('tab');
  expect(loading()).toBeVisible();
  expect(f.api.attachTerminal).not.toHaveBeenCalled();
  act(() => f.emit({ ...snapshot([session()]), revision: 2 }));
  await waitFor(() => expect(f.api.attachTerminal).toHaveBeenCalled());
  await waitFor(() => expect(loading()).not.toBeInTheDocument());
});
