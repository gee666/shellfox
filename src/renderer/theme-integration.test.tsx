// @vitest-environment jsdom
import '@testing-library/jest-dom/vitest';
import './terminal-test-mocks';
import { terminalMocks } from './terminal-test-mocks';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { App } from './App';
import { SettingsPanel } from './SettingsPanel';
import { createManagerClient } from './store';
import { TerminalRegistry, createTerminalSurface } from './terminal-client';
import { derivePalette } from './theme';
import { mockApi, profiles, snapshot } from './test-fixtures';

afterEach(() => { cleanup(); document.documentElement.removeAttribute('style'); });
beforeEach(() => localStorage.clear());

describe('background color setting', () => {
  function setup() {
    const initial = snapshot(); const fixture = mockApi(initial); const client = createManagerClient(fixture.api);
    render(<SettingsPanel client={client} settings={initial.settings} explorer={initial.explorer} cli={initial.cli} profiles={profiles} defaultProfileId="pwsh" />);
    return { fixture, initial };
  }
  it('offers dark and light presets next to a custom picker, with the default pressed', () => {
    setup();
    expect(screen.getByRole('heading', { name: 'Background color' })).toBeInTheDocument();
    expect(screen.getByLabelText('Use background #111016')).toHaveAttribute('aria-pressed', 'true');
    for (const color of ['#000000', '#ffffff', '#f5f5f7', '#fdf6e3']) expect(screen.getByLabelText(`Use background ${color}`)).toHaveAttribute('aria-pressed', 'false');
    expect(screen.getByLabelText('Background color')).toHaveValue('#111016');
  });
  it('saves a preset and a custom color through the draft mechanism, keeping the accent', async () => {
    const { fixture } = setup(); const user = userEvent.setup();
    await user.click(screen.getByLabelText('Use background #fdf6e3'));
    expect(screen.getByLabelText('Use background #fdf6e3')).toHaveAttribute('aria-pressed', 'true');
    expect(fixture.api.saveSettings).not.toHaveBeenCalled();
    fireEvent.change(screen.getByLabelText('Background color'), { target: { value: '#123456' } });
    await waitFor(() => expect(fixture.api.saveSettings).toHaveBeenCalledTimes(1));
    expect(fixture.api.saveSettings.mock.calls[0]![0]).toMatchObject({ backgroundColor: '#123456', accentColor: '#ec4899' });
    expect(screen.getByRole('status')).toHaveTextContent('Saved');
  });
  it('shows a save failure next to the background control', async () => {
    const { fixture } = setup(); const user = userEvent.setup();
    const { failure } = await import('../shared/contracts');
    fixture.api.saveSettings.mockResolvedValueOnce(failure('VALIDATION', 'Bad background.'));
    await user.click(screen.getByLabelText('Use background #ffffff'));
    const alert = await screen.findByRole('alert');
    expect(alert).toHaveTextContent('Bad background.');
    expect(alert.parentElement).toContainElement(screen.getByLabelText('Background color'));
  });
});

describe('runtime theme', () => {
  it('applies the derived palette to the document and every terminal, and follows settings changes', async () => {
    const initial = snapshot(); initial.settings.backgroundColor = '#ffffff';
    const fixture = mockApi(initial); const user = userEvent.setup();
    render(<App api={fixture.api} />);
    const root = document.documentElement;
    const light = derivePalette('#ec4899', '#ffffff');
    await waitFor(() => expect(root.style.getPropertyValue('--bg')).toBe('#ffffff'));
    expect(root.style.getPropertyValue('--fg')).toBe(light.foreground);
    expect(root.style.getPropertyValue('color-scheme')).toBe('light');
    await waitFor(() => expect(terminalMocks.terminals.length).toBeGreaterThan(0));
    expect(terminalMocks.terminals.at(-1).options.theme).toEqual(light.terminal);
    await user.click(screen.getByRole('button', { name: 'Settings' }));
    await user.click(screen.getByLabelText('Use background #111016'));
    const dark = derivePalette('#ec4899', '#111016');
    await waitFor(() => expect(root.style.getPropertyValue('--bg')).toBe('#111016'));
    expect(root.style.getPropertyValue('color-scheme')).toBe('dark');
    expect(terminalMocks.terminals.every(terminal => terminal.disposed || JSON.stringify(terminal.options.theme) === JSON.stringify(dark.terminal))).toBe(true);
  });
});

describe('TerminalRegistry.setTheme', () => {
  it('defaults to the derived default theme and updates live terminals and later ones', () => {
    const surface = createTerminalSurface(() => {}); const first = terminalMocks.terminals.at(-1);
    expect(first.options.theme).toEqual(derivePalette('#ec4899', '#111016').terminal);
    surface.setTheme!(derivePalette('#ec4899', '#ffffff').terminal);
    expect(first.options.theme.background).toBe('#ffffff');
    surface.dispose();
  });
  it('passes the current theme to surface factories and updates existing surfaces', () => {
    const themes: unknown[] = []; const updates: unknown[] = [];
    const registry = new TerminalRegistry(null, (_input, theme) => {
      themes.push(theme);
      return { element: document.createElement('div'), write: () => {}, reset() {}, fit: () => null, focus() {}, setInput() {}, setTheme: next => { updates.push(next); }, dispose() {} };
    });
    registry.getState('a'); // creates the first surface without a theme yet
    const theme = derivePalette('#ec4899', '#f5f5f7').terminal;
    registry.setTheme(theme);
    expect(themes).toEqual([undefined]); expect(updates).toEqual([theme]);
    registry.getState('b');
    expect(themes[1]).toBe(theme);
    registry.dispose();
  });
});
