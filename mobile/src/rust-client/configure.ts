import { z } from 'zod';
import type { TerminalTheme } from '../theme';

const rgb = z.string().regex(/^#[0-9a-f]{6}$/i);
export const terminalConfigurationSchema = z.object({
  foreground: rgb, background: rgb, cursor: rgb.nullable().optional(),
  palette: z.array(rgb).length(256).nullable().optional(),
  selectionBackground: rgb.optional(),
  selectionForeground: rgb.optional(),
}).strict();
export type TerminalConfiguration = z.infer<typeof terminalConfigurationSchema>;

const ansi = ['black', 'red', 'green', 'yellow', 'blue', 'magenta', 'cyan', 'white',
  'brightBlack', 'brightRed', 'brightGreen', 'brightYellow', 'brightBlue',
  'brightMagenta', 'brightCyan', 'brightWhite'] as const;

function selectionColor(value: string | undefined, background: string): string | undefined {
  if (value === undefined) return undefined;
  if (/^#[0-9a-f]{6}$/i.test(value)) return value;
  // Existing mobile theme uses #RRGGBBAA. Preblend translucent selections so
  // native renderers receive plain RGB. Accept rgba() for existing callers too.
  const hex = /^#[0-9a-f]{8}$/i.test(value);
  const match = value.match(/^rgba\((\d+),\s*(\d+),\s*(\d+),\s*(0(?:\.\d+)?|1(?:\.0+)?)\)$/);
  if (!hex && !match) throw new Error('Unsupported terminal selection color.');
  const alpha = hex ? parseInt(value.slice(7, 9), 16) / 255 : Number(match?.[4]);
  const channels = [1, 2, 3].map((index) => {
    const offset = 1 + (index - 1) * 2;
    const component = hex ? parseInt(value.slice(offset, offset + 2), 16) : Number(match?.[index]);
    if (component > 255) throw new Error('Invalid terminal selection color.');
    return Math.round(component * alpha + parseInt(background.slice(offset, offset + 2), 16) * (1 - alpha));
  });
  return '#' + channels.map((channel) => channel.toString(16).padStart(2, '0')).join('');
}

/** Maps the existing mobile preference palette, not a separate black/white theme. */
export function terminalConfiguration(theme: TerminalTheme): TerminalConfiguration {
  const background = rgb.parse(theme.background);
  const palette = ansi.map((name) => rgb.parse(theme[name]));
  const levels = [0, 95, 135, 175, 215, 255];
  for (const r of levels) for (const g of levels) for (const b of levels) {
    palette.push('#' + [r, g, b].map((channel) => channel.toString(16).padStart(2, '0')).join(''));
  }
  for (let step = 0; step < 24; step++) {
    palette.push('#' + (8 + 10 * step).toString(16).padStart(2, '0').repeat(3));
  }
  return terminalConfigurationSchema.parse({
    foreground: theme.foreground, background, cursor: theme.cursor ?? null, palette,
    selectionBackground: selectionColor(theme.selectionBackground, background),
    selectionForeground: theme.selectionForeground,
  });
}

export function encodeTerminalConfiguration(configuration: TerminalConfiguration): string {
  const json = JSON.stringify(terminalConfigurationSchema.parse(configuration));
  if (new TextEncoder().encode(json).byteLength > 16384) throw new Error('Terminal configuration exceeds limit.');
  return json;
}
