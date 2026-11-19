import { describe, expect, it } from 'vitest';
import { nativeFileLink } from './native-file-link';

describe('native terminal file links', () => {
  const parse = (text: string, offset: number, terminalHandle = '1') => nativeFileLink({ text, offset, terminalHandle }, '1');
  it('opens only the path under the touched cell and preserves its source position', () => {
    const row = '🦊 "C:\\work space\\notes.txt:20:3"  ./other.rs';
    expect(parse(row, row.indexOf('notes'))).toEqual({ path: 'C:\\work space\\notes.txt', line: 20, column: 3 });
    expect(parse(row, row.indexOf('other'))).toEqual({ path: './other.rs' });
    expect(parse(row, 2)).toBeNull();
  });
  it('resolves explicit file targets but rejects remote file authorities, non-file schemes and stale views', () => {
    expect(parse('file:///tmp/notes.txt#L4C3', -1)).toEqual({ path: '/tmp/notes.txt', line: 4, column: 3 });
    expect(parse('file://other/tmp/notes.txt', -1)).toBeNull();
    expect(parse('https://example.org/notes.txt', -1)).toBeNull();
    expect(parse('./notes.txt', 2, '2')).toBeNull();
    expect(parse('a'.repeat(8193), 1)).toBeNull();
  });
});
