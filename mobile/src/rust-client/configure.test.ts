import { describe, expect, it } from 'vitest';
import { mobileTheme } from '../theme';
import { encodeTerminalConfiguration, terminalConfiguration } from './configure';
import { encodeTerminalInput } from './input';

describe('native terminal preferences and user input', () => {
  it.each(['#111016', '#ffffff', '#808080'])('maps existing mobile palette %s without replacing preferences', (background) => {
    const theme = mobileTheme('#ffff00', background);
    const config = terminalConfiguration(theme.palette.terminal);
    expect(config.foreground).toBe(theme.colors.text);
    expect(config.background).toBe(theme.colors.bg);
    expect(config.cursor).toBe(theme.colors.accent);
    expect(config.palette).toHaveLength(256);
    expect(config.palette?.[1]).toBe(theme.palette.terminal.red);
    expect(config.palette?.[16]).toBe('#000000');
    expect(config.palette?.[231]).toBe('#ffffff');
    expect(config.palette?.[255]).toBe('#eeeeee');
    expect(config.selectionBackground).toMatch(/^#[0-9a-f]{6}$/i);
    expect(JSON.parse(encodeTerminalConfiguration(config))).toEqual(config);
  });

  it('rejects unsafe color strings and incomplete palettes', () => {
    expect(() => encodeTerminalConfiguration({
      foreground: 'red', background: '#000000',
    })).toThrow();
    expect(() => encodeTerminalConfiguration({
      foreground: '#ffffff', background: '#000000', palette: ['#000000'],
    })).toThrow();
  });

  it('sends typed keys, commits, IME preedit and paste rather than caller escapes', () => {
    expect(JSON.parse(encodeTerminalInput({
      type: 'key', key: { kind: 'arrowUp' }, action: 'press', modifiers: 2,
    }))).toEqual({ type: 'key', key: { kind: 'arrowUp' }, action: 'press', modifiers: 2 });
    expect(() => encodeTerminalInput({ type: 'commit', text: '\u001b[A' })).toThrow();
    expect(() => encodeTerminalInput({
      type: 'key', key: { kind: 'enter' }, action: 'press', modifiers: 16,
    })).toThrow();
    expect(() => encodeTerminalInput({ type: 'commit', text: '日'.repeat(25000) })).toThrow();
    expect(JSON.parse(encodeTerminalInput({ type: 'preedit', text: '日本' }))).toEqual({ type: 'preedit', text: '日本' });
  });
});
