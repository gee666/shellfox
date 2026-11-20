import { describe, expect, it } from 'vitest';
import { nativeFileLink } from './native-file-link';

describe('native terminal file links', () => {
  const parse = (text: string, terminalHandle = '1') => nativeFileLink({ text, terminalHandle }, '1');
  it('preserves source positions in resolved plaintext and OSC 8 targets', () => {
    expect(parse('C:\\work space\\notes.txt:20:3')).toEqual({ path: 'C:\\work space\\notes.txt', line: 20, column: 3 });
    expect(parse('file:///tmp/notes.txt#L4C3')).toEqual({ path: '/tmp/notes.txt', line: 4, column: 3 });
  });
  it('rejects remote file authorities, non-file schemes and stale views', () => {
    expect(parse('file://other/tmp/notes.txt')).toBeNull();
    expect(parse('https://example.org/notes.txt')).toBeNull();
    expect(parse('./notes.txt', '2')).toBeNull();
    expect(parse('a'.repeat(8193))).toBeNull();
  });
});
