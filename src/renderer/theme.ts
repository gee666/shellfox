import type { ITheme } from '@xterm/xterm';

// Pure theme derivation: every UI and terminal color comes from two inputs, the
// accent and the background. No DOM access happens here except in applyPalette.
export const DEFAULT_ACCENT = '#ec4899';
export const DEFAULT_BACKGROUND = '#111016';
const HEX = /^#[0-9a-fA-F]{6}$/;
type Rgb = [number, number, number];

export interface Palette {
  scheme: 'dark' | 'light';
  /** The chosen background, exactly. */
  background: string;
  foreground: string;
  /** Accent after contrast adjustment (hue preserved). */
  accent: string;
  /** CSS custom properties (name includes the leading --). */
  vars: Record<string, string>;
  terminal: ITheme;
}

const parse = (hex: string): Rgb => [1, 3, 5].map(index => parseInt(hex.slice(index, index + 2), 16)) as Rgb;
const clamp = (value: number) => Math.max(0, Math.min(255, Math.round(value)));
const toHex = ([r, g, b]: Rgb) => `#${[r, g, b].map(value => clamp(value).toString(16).padStart(2, '0')).join('')}`;
const channel = (value: number) => { const s = value / 255; return s <= 0.03928 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4; };
export function relativeLuminance(hex: string): number {
  const [r, g, b] = parse(hex);
  return 0.2126 * channel(r) + 0.7152 * channel(g) + 0.0722 * channel(b);
}
export function contrastRatio(a: string, b: string): number {
  const first = relativeLuminance(a), second = relativeLuminance(b);
  return (Math.max(first, second) + 0.05) / (Math.min(first, second) + 0.05);
}
/** Linear mix in sRGB space: 0 returns `a`, 1 returns `b`. */
export function mix(a: string, b: string, amount: number): string {
  const x = parse(a), y = parse(b);
  return toHex(x.map((value, index) => value + (y[index]! - value) * amount) as Rgb);
}
function toHsl(hex: string): [number, number, number] {
  const [r, g, b] = parse(hex).map(value => value / 255) as Rgb;
  const max = Math.max(r, g, b), min = Math.min(r, g, b), l = (max + min) / 2, d = max - min;
  if (!d) return [0, 0, l];
  const s = d / (1 - Math.abs(2 * l - 1));
  const h = max === r ? ((g - b) / d + 6) % 6 : max === g ? (b - r) / d + 2 : (r - g) / d + 4;
  return [h * 60, s, l];
}
/** Moves HSL lightness toward white (1) or black (0) by `amount`, keeping hue and saturation so tints survive. */
function lift(hex: string, toward: 0 | 1, amount: number): string {
  const [h, s, l] = toHsl(hex);
  return fromHsl(h, s, l + (toward - l) * Math.min(1, amount));
}
function fromHsl(h: number, s: number, l: number): string {
  const c = (1 - Math.abs(2 * l - 1)) * s, x = c * (1 - Math.abs((h / 60) % 2 - 1)), m = l - c / 2;
  const [r, g, b] = h < 60 ? [c, x, 0] : h < 120 ? [x, c, 0] : h < 180 ? [0, c, x] : h < 240 ? [0, x, c] : h < 300 ? [x, 0, c] : [c, 0, x];
  return toHex([(r + m) * 255, (g + m) * 255, (b + m) * 255]);
}
/** Moves HSL lightness (hue and saturation kept) until `test` passes; `preferred` direction first. */
function shift(hex: string, test: (candidate: string) => boolean, preferred: 'lighter' | 'darker', fallback: (candidate: string) => number): string {
  if (test(hex)) return hex;
  const [h, s, l] = toHsl(hex);
  const search = (target: number) => {
    if (!test(fromHsl(h, s, target))) return null;
    let lo = l, hi = target;
    for (let index = 0; index < 24; index++) { const mid = (lo + hi) / 2; if (test(fromHsl(h, s, mid))) hi = mid; else lo = mid; }
    return fromHsl(h, s, hi);
  };
  const order = preferred === 'lighter' ? [1, 0] : [0, 1];
  for (const target of order) { const found = search(target); if (found) return found; }
  // Unreachable in practice: return whichever extreme scores best.
  const extremes = [fromHsl(h, s, 0), fromHsl(h, s, 1)];
  return fallback(extremes[0]!) >= fallback(extremes[1]!) ? extremes[0]! : extremes[1]!;
}
/** Adjusts lightness only when `hex` has less than `min` contrast against `against`. */
export function ensureContrast(hex: string, against: string, min: number, preferred: 'lighter' | 'darker'): string {
  return shift(hex, candidate => contrastRatio(candidate, against) >= min, preferred, candidate => contrastRatio(candidate, against));
}

const withAlpha = (hex: string, alpha: number) => `${hex}${clamp(alpha * 255).toString(16).padStart(2, '0')}`;
// ANSI palettes; each entry is nudged to >= 4.5:1 against the actual background.
const DARK_ANSI = ['#000000', '#ff7b72', '#7ee787', '#e3b341', '#79c0ff', '#d2a8ff', '#56d4dd', '#d0d7de', '#6e7681', '#ffa198', '#56d364', '#f2cc60', '#a5d6ff', '#e2c5ff', '#b3f0ff', '#ffffff'];
const LIGHT_ANSI = ['#1f2328', '#cf222e', '#116329', '#7d4e00', '#0550ae', '#8250df', '#1b7c83', '#6e7781', '#57606a', '#a40e26', '#1a7f37', '#633c01', '#0969da', '#6639ba', '#136874', '#8c959f'];
const ANSI_NAMES = ['black', 'red', 'green', 'yellow', 'blue', 'magenta', 'cyan', 'white', 'brightBlack', 'brightRed', 'brightGreen', 'brightYellow', 'brightBlue', 'brightMagenta', 'brightCyan', 'brightWhite'] as const;

export function derivePalette(accentInput: string, backgroundInput: string): Palette {
  const accentBase = HEX.test(accentInput) ? accentInput.toLowerCase() : DEFAULT_ACCENT;
  const chosen = HEX.test(backgroundInput) ? backgroundInput.toLowerCase() : DEFAULT_BACKGROUND;
  // The chosen background is always used as-is. Text aims for 7:1; mid-tone
  // backgrounds where no color reaches that get the best of black/white (>= 4.58:1).
  const bg = chosen;
  const dark = contrastRatio('#ffffff', bg) >= contrastRatio('#000000', bg);
  const scheme = dark ? 'dark' : 'light';
  const preferred = dark ? 'lighter' : 'darker';
  const pole = dark ? 1 : 0;
  // Surfaces step away from the background: lighter on dark themes, darker on light ones.
  const scale = dark ? 1 : 1.5;
  const surface = (amount: number) => lift(bg, pole, amount * scale);
  const sidebar = surface(0.018), panel = surface(0.034), rowHover = surface(0.05), button = surface(0.085);
  const border = surface(0.13), hover = surface(0.15), inputBorder = surface(0.2);
  const inputBg = dark ? lift(bg, 1, 0.004) : lift(bg, 1, 0.6);

  const foreground = ensureContrast(lift(bg, pole, 0.9), bg, 7, preferred);
  const muted = ensureContrast(mix(bg, foreground, 0.7), bg, 4.5, preferred);
  const placeholder = ensureContrast(mix(bg, foreground, 0.45), inputBg, 3, preferred);
  // Small accent text needs stronger contrast on light backgrounds. Keep dark themes unchanged.
  const accent = ensureContrast(accentBase, bg, dark ? 3 : 4.5, preferred);
  const onAccent = contrastRatio('#ffffff', accent) >= 3 || contrastRatio('#ffffff', accent) >= contrastRatio('#000000', accent) ? '#ffffff' : '#000000';
  const status = (color: string) => ensureContrast(color, bg, 3, preferred);
  const [shell, running, error] = [status('#60a5fa'), status('#4ade80'), status('#fc8397')];
  const [r, g, b] = parse(accent);

  const ansi = (dark ? DARK_ANSI : LIGHT_ANSI).map(color => ensureContrast(color, bg, 4.5, preferred));
  // Colors that naturally blend with the background (black on dark, white on light) stay as surface-like boxes.
  if (dark) ansi[0] = mix(bg, foreground, 0.15);
  else { ansi[7] = mix(bg, foreground, 0.15); ansi[15] = mix(bg, foreground, 0.06); }
  const terminal: ITheme = {
    background: bg, foreground, cursor: accent, cursorAccent: bg,
    selectionBackground: withAlpha(accent, dark ? 0.4 : 0.3),
    scrollbarSliderBackground: withAlpha(border, 0.5), scrollbarSliderHoverBackground: inputBorder, scrollbarSliderActiveBackground: inputBorder,
    overviewRulerBorder: bg,
  };
  ANSI_NAMES.forEach((name, index) => { terminal[name] = ansi[index]; });

  return {
    scheme, background: bg, foreground, accent, terminal,
    vars: {
      '--bg': bg, '--fg': foreground, '--muted': muted, '--placeholder': placeholder,
      '--border': border, '--panel': panel, '--sidebar': sidebar, '--surface': button, '--hover': hover, '--row-hover': rowHover,
      '--input-bg': inputBg, '--input-border': inputBorder,
      '--accent': accent, '--accent-rgb': `${r}, ${g}, ${b}`, '--on-accent': onAccent,
      '--brand-opacity': dark ? '.72' : '1', '--brand-label-opacity': dark ? '.6' : '1',
      '--dot-shell': shell, '--dot-running': running, '--dot-error': error,
      '--shadow': dark ? 'rgba(0, 0, 0, .4)' : 'rgba(0, 0, 0, .18)',
      '--shadow-strong': dark ? 'rgba(0, 0, 0, .53)' : 'rgba(0, 0, 0, .28)',
      '--backdrop': dark ? 'rgba(0, 0, 0, .53)' : 'rgba(0, 0, 0, .35)',
    },
  };
}

export function applyPalette(palette: Palette, root: HTMLElement = document.documentElement) {
  for (const [name, value] of Object.entries(palette.vars)) root.style.setProperty(name, value);
  root.style.setProperty('color-scheme', palette.scheme);
}
