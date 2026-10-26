import { z } from 'zod';

export type ColorKind = 'accent' | 'background';
export const colorSchema = z.string().regex(/^#[0-9a-f]{6}$/i).transform(color => color.toLowerCase());
export const ACCENT_PRESETS = ['#ec4899', '#a78bfa', '#60a5fa', '#2dd4bf', '#4ade80', '#fbbf24', '#fb923c', '#fc8397'];
export const BACKGROUND_PRESETS = ['#111016', '#000000', '#1e1e2e', '#0f172a', '#ffffff', '#f5f5f7', '#fdf6e3'];
export const presetsFor = (kind: ColorKind): readonly string[] => kind === 'accent' ? ACCENT_PRESETS : BACKGROUND_PRESETS;

// Match desktop swatches: normalize, deduplicate, exclude presets, keep the last three.
export function customColors(kind: ColorKind, values: readonly string[]): string[] {
  return [...new Set(values.map(value => value.toLowerCase()).filter(value => colorSchema.safeParse(value).success && !presetsFor(kind).includes(value)))].slice(-3);
}
export function saveCustomColor(kind: ColorKind, values: readonly string[], value: string): string[] {
  return customColors(kind, [...values, value]);
}
