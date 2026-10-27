import { describe, expect, it } from 'vitest';
import { TerminalQueryFilter } from './terminal-queries';
import { Terminal } from '@xterm/headless';

const hostQueries = ['\x1b[c', '\x1b[0c', '\x1b[>0c', '\x1b[5n', '\x1b[6n', '\x1b[?6n', '\x1b[1$p', '\x1b[?25$p', '\x1bP$qm\x1b\\', '\x1b[18t'];
const browserQueries = ['\x1b[14t', '\x1b[16t', '\x1b]10;?\x07', '\x1b]4;1;?\x1b\\'];
const render = (parts: string[], historical: boolean) => {
  const filter = new TerminalQueryFilter();
  return parts.map(part => filter.push(part, historical)).join('');
};
describe('external terminal query ownership', () => {
  it.each(hostQueries)('never asks a second display to answer a host-owned query %j', query => {
    expect(render(['before', query, 'after'], false)).toBe('beforeafter');
    expect(render(['before', ...query, 'after'], false)).toBe('beforeafter');
    expect(render(['before', ...query, 'after'], true)).toBe('beforeafter');
  });
  it.each(browserQueries)('keeps live display reports but removes historical queries %j', query => {
    expect(render([...query], false)).toBe(query);
    expect(render([...query], true)).toBe('');
  });
  it('keeps a real second terminal parser from generating duplicate device replies', async () => {
    const raw = new Terminal({ cols: 80, rows: 24, allowProposedApi: true }), filtered = new Terminal({ cols: 80, rows: 24, allowProposedApi: true });
    const originalReplies: string[] = [], duplicateReplies: string[] = [];
    raw.onData(reply => originalReplies.push(reply)); filtered.onData(reply => duplicateReplies.push(reply));
    const output = 'steady\x1b[c\x1b[>c\x1b[6n\x1bP$qm\x1b\\';
    try {
      await new Promise<void>(resolve => raw.write(output, resolve));
      expect(originalReplies.length).toBeGreaterThan(0);
      const filter = new TerminalQueryFilter();
      for (const character of output) {
        const part = filter.push(character, false);
        if (part) await new Promise<void>(resolve => filtered.write(part, resolve));
      }
      expect(duplicateReplies).toEqual([]);
      expect(filtered.buffer.active.getLine(0)?.translateToString(true)).toBe('steady');
    } finally { raw.dispose(); filtered.dispose(); }
  });
  it('preserves printable text, colors, cursor addressing, alternate screen and palette changes byte-for-byte', () => {
    const stream = '雪🙂\r\n\x1b[?1049h\x1b[38;2;23;45;67mcolor\x1b[2;3Hcursor\x1b[?25l\x1b[2J\x1b]4;1;rgb:ff/00/00\x07\x1b]0;title\x1b\\\x1bP1;2qimage\x1b\\\x1b[?1049l';
    expect(render([stream], true)).toBe(stream);
    expect(render([...stream], false)).toBe(stream);
  });
  it('retains the original replay classification when a query spans replay and live chunks', () => {
    const filter = new TerminalQueryFilter();
    expect(filter.push('first\x1b]10;', true)).toBe('first');
    expect(filter.push('?\x07live', false)).toBe('live');
  });
  it('passes large non-query control payloads without retaining an unbounded string', () => {
    const filter = new TerminalQueryFilter(), prefix = '\x1b]0;' + 'title'.repeat(2000);
    const first = filter.push(prefix, false);
    expect(first.length).toBeGreaterThan(4096);
    expect(first + filter.push('rest\x1b\\after', false)).toBe(prefix + 'rest\x1b\\after');
  });
  it('drops incomplete old parser state when ownership changes', () => {
    const filter = new TerminalQueryFilter();
    expect(filter.push('\x1b]0;old partial', false)).toBe('');
    filter.reset();
    expect(filter.push('new screen', true)).toBe('new screen');
  });
  it.each(['\x1b[\x1b[6n', '\x1b[\x07c'])('suppresses restarted and C0-interrupted queries %j', query => {
    expect(render([query], false).replaceAll('\x07', '')).toBe('');
    expect(render([...query], false).replaceAll('\x07', '')).toBe('');
  });
  it('never leaks recognized long DECRQSS queries through the bounded-buffer fallback', () => {
    const query = '\x1bP$q' + 'm'.repeat(10000) + '\x1b\\';
    expect(render([query], false)).toBe('');
    expect(render([...query], false)).toBe('');
  });
  it.each(['\x1b]0;unfinished\x18READY', '\x1bP$qunfinished\x1aREADY'])('cancels a control string and immediately resumes text %j', stream => {
    expect(render([stream], false).replace(/[\x18\x1a]/g, '')).toBe('READY');
    expect(render([...stream], false).replace(/[\x18\x1a]/g, '')).toBe('READY');
  });
  it('preserves mixed palette settings while removing only historical report requests', () => {
    const mixed = '\x1b]4;1;rgb:ff/00/00;2;?\x07';
    expect(render([...mixed], true)).toBe('\x1b]4;1;rgb:ff/00/00\x07');
    expect(render([mixed], false)).toBe(mixed);
    expect(render([...('\x1b]10;?;rgb:00/00/00;rgb:ff/ff/ff\x1b\\')], true)).toBe('\x1b]11;rgb:00/00/00\x1b\\\x1b]12;rgb:ff/ff/ff\x1b\\');
  });
  it('does not leak remote ConPTY input mode or window resizes into the external console', () => {
    const stream = 'before\x1b[?9001h\x1b[8;45;117t\x1b[4;800;1200tafter';
    expect(render([stream], false)).toBe('beforeafter');
    expect(render([...stream], true)).toBe('beforeafter');
    expect(render([...('\x1b[?25;9001;1004h')], false)).toBe('\x1b[?25;1004h');
  });
  it('handles C1 controls without damaging subsequent text', () => {
    expect(render(['a\u009b6nb\u0090$qm\u009cc'], false)).toBe('abc');
    expect(render(['\u009d0;title\u009c'], true)).toBe('\u009d0;title\u009c');
  });
});
