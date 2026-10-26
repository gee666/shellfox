import { describe, expect, it } from 'vitest';
import { ACCENT_PRESETS, BACKGROUND_PRESETS, colorSchema, customColors, saveCustomColor } from './color-settings';
import { hexToHsv, hsvToHex } from './color-picker';

describe('mobile colors', () => {
  it('uses the desktop presets in the same order', () => {
    expect(ACCENT_PRESETS).toEqual(['#ec4899', '#a78bfa', '#60a5fa', '#2dd4bf', '#4ade80', '#fbbf24', '#fb923c', '#fc8397']);
    expect(BACKGROUND_PRESETS).toEqual(['#111016', '#000000', '#1e1e2e', '#0f172a', '#ffffff', '#f5f5f7', '#fdf6e3']);
  });
  it('keeps the last three unique non-preset colors, like desktop', () => {
    expect(customColors('accent', ['bad', '#EC4899', '#ABCDEF', '#abcdef', '#112233', '#445566', '#778899'])).toEqual(['#112233', '#445566', '#778899']);
    expect(customColors('background', ['#FFFFFF', '#111016', '#123456'])).toEqual(['#123456']);
    expect(saveCustomColor('accent', ['#123456', '#234567', '#345678'], '#456789')).toEqual(['#234567', '#345678', '#456789']);
  });
  it('does not reorder duplicates or save a preset', () => {
    const saved = ['#123456', '#234567', '#345678'];
    expect(saveCustomColor('accent', saved, '#123456')).toEqual(saved);
    expect(saveCustomColor('accent', saved, '#ec4899')).toEqual(saved);
  });
  it.each(['#000000', '#ffffff', '#ec4899', '#111016', '#123456', '#00ff00', '#0000ff', '#ff0000'])('round trips picker color %s', hex => {
    expect(hsvToHex(hexToHsv(hex))).toBe(hex);
  });
  it('normalizes valid six-digit colors only', () => {
    expect(colorSchema.parse('#ABCDEF')).toBe('#abcdef');
    for (const value of ['red', '#fff', '#12345678', '123456']) expect(colorSchema.safeParse(value).success).toBe(false);
  });
});
