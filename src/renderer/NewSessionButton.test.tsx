// @vitest-environment jsdom
import '@testing-library/jest-dom/vitest';
import { cleanup, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, expect, it, vi } from 'vitest';
import { NewSessionButton } from './NewSessionButton';
import { remoteFixture } from './remote-test-fixtures';
afterEach(cleanup);
it('opens the local prompt directly when no remote machines are configured', async () => {
  const choose = vi.fn(); render(<NewSessionButton peers={[]} selectedMachine="local" localAvailable busy={false} onChoose={choose} />);
  await userEvent.click(screen.getByRole('button', { name: 'New session' }));
  expect(choose).toHaveBeenCalledExactlyOnceWith('local'); expect(screen.queryByRole('menu')).not.toBeInTheDocument();
});
it('offers machine names directly under the plus, without creating anything until a choice', async () => {
  const { state } = remoteFixture(), choose = vi.fn();
  render(<NewSessionButton peers={state.peers} selectedMachine="workstation" localAvailable busy={false} onChoose={choose} />);
  const button = screen.getByRole('button', { name: 'New session' });
  vi.spyOn(button, 'getBoundingClientRect').mockReturnValue(new DOMRect(100, 20, 28, 28));
  await userEvent.click(button);
  expect(screen.getByRole('menu').style.top).toBe('53px'); expect(choose).not.toHaveBeenCalled();
  expect(screen.getByRole('menuitem', { name: /Workstation/ })).toHaveFocus();
  expect(screen.getByRole('menuitem', { name: /Linux server/ })).toBeDisabled();
  await userEvent.click(screen.getByRole('menuitem', { name: 'Local' }));
  expect(choose).toHaveBeenCalledExactlyOnceWith('local'); expect(screen.queryByRole('menu')).not.toBeInTheDocument();
});
it('routes a remote choice explicitly and supports Escape and keyboard selection', async () => {
  const { state } = remoteFixture(), choose = vi.fn(), user = userEvent.setup();
  render(<NewSessionButton peers={state.peers} selectedMachine="local" localAvailable busy={false} onChoose={choose} />);
  const plus = screen.getByRole('button', { name: 'New session' }); await user.click(plus);
  await user.keyboard('{Escape}'); expect(plus).toHaveFocus(); expect(choose).not.toHaveBeenCalled();
  await user.click(plus); await user.keyboard('{End}{Enter}');
  expect(choose).toHaveBeenCalledExactlyOnceWith('workstation');
});
it('allows remote creation when this machine has no local shell', async () => {
  const { state } = remoteFixture(), choose = vi.fn();
  render(<NewSessionButton peers={state.peers} selectedMachine="local" localAvailable={false} busy={false} onChoose={choose} />);
  await userEvent.click(screen.getByRole('button', { name: 'New session' }));
  expect(screen.getByRole('menuitem', { name: 'Local' })).toBeDisabled();
  await userEvent.click(screen.getByRole('menuitem', { name: /Workstation/ })); expect(choose).toHaveBeenCalledWith('workstation');
});
