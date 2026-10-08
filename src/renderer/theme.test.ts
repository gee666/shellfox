import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';
import { DEFAULT_ACCENT, DEFAULT_BACKGROUND, applyPalette, contrastRatio, derivePalette, ensureContrast, relativeLuminance } from './theme';

const BACKGROUNDS = ['#000000', '#ffffff', '#777777', '#808080', '#111016', '#f5f5f7', '#fdf6e3', '#1e1e2e', '#0f172a',
  '#ff0000', '#00ff00', '#0000ff', '#ffff00', '#00ffff', '#ff00ff', '#8b0000', '#006400', '#00008b', '#c0c0c0', '#404040', '#555555', '#999999', '#a0522d', '#ffd700'];
const ACCENTS = [DEFAULT_ACCENT, '#111016', '#ffffff', '#000000', '#fbbf24', '#808080', '#0000ff'];
const ANSI = ['black', 'red', 'green', 'yellow', 'blue', 'magenta', 'cyan', 'white', 'brightBlack', 'brightRed', 'brightGreen', 'brightYellow', 'brightBlue', 'brightMagenta', 'brightCyan', 'brightWhite'] as const;

describe('contrast math', () => {
  it('matches WCAG reference values', () => {
    expect(relativeLuminance('#ffffff')).toBeCloseTo(1, 5);
    expect(relativeLuminance('#000000')).toBeCloseTo(0, 5);
    expect(contrastRatio('#000000', '#ffffff')).toBeCloseTo(21, 3);
    expect(contrastRatio('#777777', '#ffffff')).toBeCloseTo(4.48, 1);
  });
  it('ensureContrast keeps hue and only moves lightness when needed', () => {
    expect(ensureContrast('#60a5fa', '#111016', 3, 'lighter')).toBe('#60a5fa');
    const adjusted = ensureContrast('#60a5fa', '#ffffff', 4.5, 'darker');
    expect(contrastRatio(adjusted, '#ffffff')).toBeGreaterThanOrEqual(4.5);
    const [r, g, b] = [1, 3, 5].map(i => parseInt(adjusted.slice(i, i + 2), 16));
    expect(b!).toBeGreaterThan(g!); expect(g!).toBeGreaterThan(r!);
  });
});

describe.each(BACKGROUNDS)('palette on background %s', background => {
  const palette = derivePalette(DEFAULT_ACCENT, background);
  const bg = palette.background;
  it('picks a scheme and contrasting text automatically', () => {
    // The best a text color can do is black or white; aim for 7:1, never below what is reachable.
    const best = Math.max(contrastRatio('#ffffff', bg), contrastRatio('#000000', bg));
    expect(best).toBeGreaterThanOrEqual(4.58);
    expect(contrastRatio(palette.foreground, bg)).toBeGreaterThanOrEqual(Math.min(7, best) - 0.01);
    expect(contrastRatio(palette.vars['--fg']!, bg)).toBeGreaterThanOrEqual(Math.min(7, best) - 0.01);
    expect(contrastRatio(palette.vars['--muted']!, bg)).toBeGreaterThanOrEqual(Math.min(4.5, best) - 0.01);
    expect(palette.scheme).toBe(contrastRatio('#ffffff', bg) >= contrastRatio('#000000', bg) ? 'dark' : 'light');
    // Light backgrounds get dark text and vice versa.
    expect(relativeLuminance(palette.foreground) < relativeLuminance(bg)).toBe(palette.scheme === 'light');
  });
  it('always keeps the exact chosen background', () => {
    expect(bg).toBe(background.toLowerCase());
    expect(palette.terminal.background).toBe(background.toLowerCase());
  });
  it('keeps status colors visible', () => {
    for (const name of ['--dot-shell', '--dot-running', '--dot-error']) expect(contrastRatio(palette.vars[name]!, bg)).toBeGreaterThanOrEqual(3);
  });
  it('derives distinguishable surfaces in the right direction', () => {
    const sign = palette.scheme === 'dark' ? 1 : -1;
    const luminance = (name: string) => relativeLuminance(palette.vars[name]!);
    for (const name of ['--sidebar', '--panel', '--row-hover', '--surface', '--border', '--hover', '--input-border']) {
      if (background === '#000000' || background !== '#ffffff' || name !== '--sidebar') expect((luminance(name) - relativeLuminance(bg)) * sign).toBeGreaterThan(0);
    }
    expect(palette.vars['--panel']).not.toBe(bg);
    expect(palette.vars['--border']).not.toBe(palette.vars['--panel']);
    expect(palette.vars['--hover']).not.toBe(palette.vars['--surface']);
  });
  it('builds a readable terminal theme', () => {
    const { terminal } = palette;
    expect(terminal.background).toBe(bg);
    expect(terminal.foreground).toBe(palette.foreground);
    expect(terminal.cursorAccent).toBe(bg);
    expect(terminal.cursor).toBe(palette.accent);
    expect(terminal.selectionBackground).toMatch(/^#[0-9a-f]{8}$/);
    expect(terminal.selectionBackground!.slice(0, 7)).toBe(palette.accent);
    // Black on dark and white on light backgrounds intentionally blend into the background.
    const exempt = new Set<string>(palette.scheme === 'dark' ? ['black'] : ['white', 'brightWhite']);
    for (const name of ANSI) {
      expect(terminal[name], name).toMatch(/^#[0-9a-f]{6}$/);
      if (!exempt.has(name)) expect(contrastRatio(terminal[name]!, bg), name).toBeGreaterThanOrEqual(4.5);
    }
    expect(new Set(ANSI.map(name => terminal[name])).size).toBeGreaterThanOrEqual(14);
  });
  it('uses shadows appropriate for the scheme', () => {
    const alpha = (name: string) => Number(`0.${/\.(\d+)\)/.exec(palette.vars[name]!)![1]}`);
    if (palette.scheme === 'light') expect(alpha('--shadow')).toBeLessThan(0.3);
    else expect(alpha('--shadow')).toBeGreaterThanOrEqual(0.3);
  });
});

describe('accent adjustment', () => {
  it.each(ACCENTS.flatMap(accent => BACKGROUNDS.map(background => [accent, background] as const)))('accent %s on %s has at least 3:1', (accent, background) => {
    const palette = derivePalette(accent, background);
    expect(contrastRatio(palette.accent, palette.background)).toBeGreaterThanOrEqual(3);
    expect(palette.vars['--accent']).toBe(palette.accent);
    expect(palette.terminal.cursor).toBe(palette.accent);
    const [r, g, b] = [1, 3, 5].map(i => parseInt(palette.accent.slice(i, i + 2), 16));
    expect(palette.vars['--accent-rgb']).toBe(`${r}, ${g}, ${b}`);
  });
  it.each(ACCENTS.flatMap(accent => BACKGROUNDS.map(background => [accent, background] as const)))('keeps dark styling and strengthens light contrast for %s on %s', (accent, background) => {
    const palette = derivePalette(accent, background);
    if (palette.scheme === 'dark') {
      expect(palette.accent).toBe(ensureContrast(accent, background, 3, 'lighter'));
      expect(palette.vars['--brand-opacity']).toBe('.72');
      expect(palette.vars['--brand-label-opacity']).toBe('.6');
    } else {
      expect(contrastRatio(palette.accent, background)).toBeGreaterThanOrEqual(4.5);
      expect(palette.vars['--brand-opacity']).toBe('1');
      expect(palette.vars['--brand-label-opacity']).toBe('1');
    }
  });
  it('preserves a dark accent on light backgrounds instead of fading it', () => {
    expect(derivePalette('#880044', '#ffffff').accent).toBe('#880044');
  });
  it('leaves a sufficiently contrasting accent untouched and keeps the hue otherwise', () => {
    expect(derivePalette('#ec4899', '#111016').accent).toBe('#ec4899');
    const adjusted = derivePalette('#ec4899', '#ffb6d5').accent;
    expect(adjusted).not.toBe('#ec4899');
    const [r, g, b] = [1, 3, 5].map(i => parseInt(adjusted.slice(i, i + 2), 16));
    expect(r!).toBeGreaterThan(g!); expect(b!).toBeGreaterThan(g!);
    expect(derivePalette('#111016', '#111016').accent).not.toBe('#111016');
  });
  it('chooses a readable color for content on accent fills', () => {
    expect(derivePalette('#ec4899', '#111016').vars['--on-accent']).toBe('#ffffff');
    const light = derivePalette('#fde047', '#111016');
    expect(contrastRatio(light.vars['--on-accent']!, light.accent)).toBeGreaterThanOrEqual(7);
  });
});

describe('defaults and robustness', () => {
  it('keeps the default status colors and accent', () => {
    const { vars, scheme } = derivePalette(DEFAULT_ACCENT, DEFAULT_BACKGROUND);
    expect(scheme).toBe('dark');
    expect(vars).toMatchObject({ '--bg': '#111016', '--accent': '#ec4899', '--dot-shell': '#60a5fa', '--dot-running': '#4ade80', '--dot-error': '#fc8397' });
  });
  it('falls back to defaults for malformed colors', () => {
    expect(derivePalette('pink', 'red!')).toEqual(derivePalette(DEFAULT_ACCENT, DEFAULT_BACKGROUND));
    expect(derivePalette('#EC4899', '#111016')).toEqual(derivePalette('#ec4899', '#111016'));
  });
  it('is deterministic', () => {
    expect(derivePalette('#22aa88', '#fdf6e3')).toEqual(derivePalette('#22aa88', '#fdf6e3'));
  });
  it('applies every variable and the color scheme to an element', () => {
    const properties = new Map<string, string>();
    const root = { style: { setProperty: (name: string, value: string) => { properties.set(name, value); } } } as unknown as HTMLElement;
    const palette = derivePalette('#ec4899', '#ffffff');
    applyPalette(palette, root);
    for (const [name, value] of Object.entries(palette.vars)) expect(properties.get(name)).toBe(value);
    expect(properties.get('color-scheme')).toBe('light');
  });
});

describe('stylesheets', () => {
  const read = (name: string) => readFileSync(new URL(`./${name}`, import.meta.url), 'utf8');
  const css = { 'styles.css': read('styles.css'), 'terminal.css': read('terminal.css') };
  it('keeps the :root fallback variables identical to the default palette', () => {
    const root = /:root\s*{([^}]*)}/.exec(css['styles.css'])![1]!;
    const palette = derivePalette(DEFAULT_ACCENT, DEFAULT_BACKGROUND);
    for (const [name, value] of Object.entries(palette.vars)) expect(root, name).toContain(`${name}: ${value};`);
  });
  it('declares every variable the palette provides and uses no hard-coded theme colors elsewhere', () => {
    for (const [file, text] of Object.entries(css)) {
      const outsideRoot = text.replace(/:root\s*{[^}]*}/, '');
      expect(outsideRoot.match(/#[0-9a-fA-F]{3,8}\b|rgba?\((?!var)[^)]*\)/g) ?? [], file).toEqual([]);
    }
  });
  it('only references variables that exist', () => {
    const known = new Set(Object.keys(derivePalette(DEFAULT_ACCENT, DEFAULT_BACKGROUND).vars));
    known.add('--sidebar-width');
    for (const text of Object.values(css)) for (const [, name] of text.matchAll(/var\((--[a-z-]+)/g)) expect(known.has(name!), name).toBe(true);
  });
});
