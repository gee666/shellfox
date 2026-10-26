// @vitest-environment jsdom
import React from 'react';
import { cleanup, render, screen } from '@testing-library/react';
import { afterEach, describe, expect, it } from 'vitest';
import { contrastRatio, derivePalette } from '../../src/renderer/theme';
import { DEFAULT_ACCENT, DEFAULT_BACKGROUND, mobileTheme, ThemeProvider, useTheme } from './theme';

afterEach(cleanup);

describe('mobile theme', () => {
  it('uses desktop defaults for legacy preferences', () => {
    expect(mobileTheme().palette).toEqual(derivePalette(DEFAULT_ACCENT, DEFAULT_BACKGROUND));
  });
  it.each(['#111016', '#ffffff', '#808080', '#0f172a', '#fdf6e3'])('uses desktop contrast, surfaces, semantic colors and terminal palette for %s', background => {
    const { palette, colors } = mobileTheme('#ffff00', background);
    expect(palette).toEqual(derivePalette('#ffff00', background));
    expect(colors.bg).toBe(background);
    expect(colors.panel).toBe(palette.vars['--panel']);
    expect(colors.onAccent).toBe(palette.vars['--on-accent']);
    expect(colors.danger).toBe(palette.vars['--dot-error']);
    expect(colors.green).toBe(palette.vars['--dot-running']);
    expect(colors.shell).toBe(palette.vars['--dot-shell']);
    expect(colors.settled).toBe(palette.vars['--dot-settled']);
    expect(contrastRatio(colors.text, colors.bg)).toBeGreaterThanOrEqual(4.5);
    expect(palette.terminal.background).toBe(colors.bg);
    expect(palette.terminal.foreground).toBe(colors.text);
    expect(palette.terminal.cursor).toBe(colors.accent);
  });
  it('does not turn status colors into the selected accent', () => {
    const first = mobileTheme('#ec4899');
    const next = mobileTheme('#60a5fa');
    expect(first.colors.green).toBe(next.colors.green);
    expect(first.colors.danger).toBe(next.colors.danger);
    expect(first.colors.shell).toBe(next.colors.shell);
    expect(first.colors.settled).toBe(next.colors.settled);
  });
  it('updates mounted app and terminal consumers without remounting', () => {
    let mounts = 0;
    function Consumer() {
      const { colors, palette } = useTheme();
      React.useEffect(() => { mounts++; }, []);
      return <output>{JSON.stringify({ colors, terminal: palette.terminal, scheme: palette.scheme })}</output>;
    }
    const view = render(<ThemeProvider><Consumer /></ThemeProvider>);
    expect(screen.getByRole('status').textContent).toContain(DEFAULT_BACKGROUND);
    view.rerender(<ThemeProvider accentColor="#0000ff" backgroundColor="#ffffff"><Consumer /></ThemeProvider>);
    expect(screen.getByRole('status').textContent).toBe(JSON.stringify({ colors: mobileTheme('#0000ff', '#ffffff').colors, terminal: mobileTheme('#0000ff', '#ffffff').palette.terminal, scheme: 'light' }));
    expect(mounts).toBe(1);
  });
});
