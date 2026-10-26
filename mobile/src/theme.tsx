import React, { createContext, useContext, useMemo } from 'react';
import { DEFAULT_ACCENT, DEFAULT_BACKGROUND, derivePalette, type Palette } from '../../src/renderer/theme';

export { DEFAULT_ACCENT, DEFAULT_BACKGROUND };

export function mobileTheme(accentColor = DEFAULT_ACCENT, backgroundColor = DEFAULT_BACKGROUND) {
  const palette = derivePalette(accentColor, backgroundColor);
  const variable = (name: string): string => {
    const value = palette.vars[name];
    if (value === undefined) throw new Error(`Missing theme color ${name}`);
    return value;
  };
  return {
    palette,
    colors: {
      bg: palette.background,
      panel: variable('--panel'),
      line: variable('--border'),
      text: palette.foreground,
      muted: variable('--muted'),
      accent: palette.accent,
      onAccent: variable('--on-accent'),
      danger: variable('--dot-error'),
      green: variable('--dot-running'),
      shell: variable('--dot-shell'),
      settled: variable('--dot-settled'),
      surface: variable('--surface'),
      input: variable('--input-bg'),
      inputBorder: variable('--input-border'),
      placeholder: variable('--placeholder'),
      backdrop: variable('--backdrop'),
    },
  };
}
export type MobileTheme = ReturnType<typeof mobileTheme>;
export type ThemeColors = MobileTheme['colors'];
export type TerminalTheme = Palette['terminal'];
const ThemeContext = createContext(mobileTheme());

export function ThemeProvider({ accentColor, backgroundColor, children }: {
  accentColor?: string;
  backgroundColor?: string;
  children: React.ReactNode;
}) {
  const value = useMemo(() => mobileTheme(accentColor, backgroundColor), [accentColor, backgroundColor]);
  return <ThemeContext.Provider value={value}>{children}</ThemeContext.Provider>;
}

export function useTheme(): MobileTheme {
  return useContext(ThemeContext);
}
