import { describe, expect, it } from 'vitest';
import { allowTerminalNavigation, controlInput, hostMessageSchema, parsePageMessage, terminalFontSize } from './protocol';
const identity = { viewId: 'page-1', epoch: '1' };
describe('mobile terminal bridge protocol', () => {
  it('rejects malformed, excessive and unknown web messages', () => {
    for (const value of ['{', 'null', JSON.stringify({ type: 'input', ...identity, data: 'x', command: 'eval' }),
      JSON.stringify({ type: 'resize', ...identity, cols: 501, rows: 24 }),
      JSON.stringify({ type: 'rendered', ...identity, ticket: -1 }),
      JSON.stringify({ type: 'input', ...identity, data: '\0' }),
      JSON.stringify({ type: 'input', ...identity, data: 'é'.repeat(32769) }), 'x'.repeat(400001)]) expect(parsePageMessage(value)).toBeNull();
  });
  it('accepts IME input, resize, selection and render receipts', () => {
    const message = { type: 'input', ...identity, data: '日本語\r' };
    expect(parsePageMessage(JSON.stringify(message))).toEqual(message);
    expect(parsePageMessage(JSON.stringify({ type: 'resize', ...identity, cols: 80, rows: 24 }))).not.toBeNull();
    expect(parsePageMessage(JSON.stringify({ type: 'rendered', ...identity, ticket: 5 }))).not.toBeNull();
    expect(parsePageMessage(JSON.stringify({ type: 'selection', ...identity, text: 'copy me' }))).not.toBeNull();
  });
  it('translates sticky Ctrl without changing terminal escape sequences or Unicode', () => {
    expect(controlInput('c', true)).toBe('\x03'); expect(controlInput('D', true)).toBe('\x04');
    expect(controlInput('[', true)).toBe('\x1b'); expect(controlInput('?', true)).toBe('\x7f');
    expect(controlInput('\x1b[A', true)).toBe('\x1b[A'); expect(controlInput('日本語', true)).toBe('日本語');
    expect(controlInput('c', false)).toBe('c');
  });
  it('keeps terminal output as data, not executable source', () => {
    const data = '</script><script>fetch("https://evil.invalid")</script>';
    const message = { type: 'output', ...identity, ticket: 1, historical: false, fit: false, data };
    expect(hostMessageSchema.parse(JSON.parse(JSON.stringify(message)))).toEqual(message);
    expect(hostMessageSchema.safeParse({ type: 'eval', ...identity, code: data }).success).toBe(false);
  });
  it('bounds fonts and blocks remote, data, file and script navigation', () => {
    expect([terminalFontSize(2), terminalFontSize(80), terminalFontSize(NaN)]).toEqual([8, 32, 14]);
    expect(allowTerminalNavigation('about:blank')).toBe(true);
    for (const url of ['https://example.com', 'http://localhost', 'file:///tmp/a', 'data:text/html,x', 'javascript:alert(1)', 'about:blank#x']) expect(allowTerminalNavigation(url)).toBe(false);
  });
});
