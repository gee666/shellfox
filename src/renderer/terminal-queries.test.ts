import { Terminal } from '@xterm/headless';
import { describe, expect, it } from 'vitest';
import { installTerminalQueryHandlers } from './terminal-queries';

const parse = (term: Terminal, data: string) => new Promise<void>(resolve => term.write(data, resolve));
const queries = ['\x1b[c', '\x1b[>c', '\x1b[5n', '\x1b[6n', '\x1b[?6n', '\x1b[4$p', '\x1b[?2004$p', '\x1bP$qm\x1b\\', '\x1b[18t'];
describe('renderer VT query ownership', () => {
  it.each(queries)('does not answer a backend-owned query: %j', async query => {
    const term = new Terminal({ cols: 80, rows: 24, allowProposedApi: true, windowOptions: { getWinSizeChars: true } });
    const replies: string[] = []; term.onData(data => replies.push(data));
    await parse(term, query); expect(replies.length).toBeGreaterThan(0);
    replies.length = 0;
    const handlers = installTerminalQueryHandlers(term.parser, () => false);
    await parse(term, query); expect(replies).toEqual([]);
    handlers.dispose(); term.dispose();
  });

  it('does not suppress keyboard, mouse, focus, or paste input interleaved with parsing', async () => {
    const term = new Terminal({ allowProposedApi: true });
    const handlers = installTerminalQueryHandlers(term.parser, () => true);
    const input: string[] = []; term.onData(data => input.push(data));
    const writing = parse(term, queries.join(''));
    for (const data of ['a', '\x1b[<0;4;2M', '\x1b[I', '\x1b[200~paste\x1b[201~']) term.input(data, true);
    await writing;
    expect(input).toEqual(['a', '\x1b[<0;4;2M', '\x1b[I', '\x1b[200~paste\x1b[201~']);
    handlers.dispose(); term.dispose();
  });

  it('leaves live browser-only reports and non-query state changes to xterm, but consumes historical reports', async () => {
    const term = new Terminal({ allowProposedApi: true, windowOptions: { getWinSizePixels: true, getCellSizePixels: true, pushTitle: true } });
    const browser: string[] = [];
    // Public fallback handlers stand in for DOM-only color/pixel reporting.
    const fallbacks = [4, 10, 11, 12].map(id => term.parser.registerOscHandler(id, data => { browser.push(`${id}:${data}`); return true; }));
    fallbacks.push(term.parser.registerCsiHandler({ final: 't' }, params => { browser.push(`t:${params[0]}`); return true; }));
    let historical = false;
    const handlers = installTerminalQueryHandlers(term.parser, () => historical);
    const reports = '\x1b]4;1;?\x07\x1b]10;?\x07\x1b]11;?\x07\x1b]12;?\x07\x1b[14t\x1b[16t';
    await parse(term, reports);
    expect(browser).toEqual(['4:1;?', '10:?', '11:?', '12:?', 't:14', 't:16']);
    browser.length = 0; historical = true;
    await parse(term, reports); expect(browser).toEqual([]);
    await parse(term, '\x1b]4;1;rgb:ff/00/00;2;?\x07');
    expect(browser).toEqual([]); // Mixed historical sets/reports are intentionally consumed together.
    await parse(term, '\x1b]4;1;rgb:ff/00/00\x07\x1b]10;rgb:ff/ff/ff\x07\x1b[22;0t');
    expect(browser).toEqual(['4:1;rgb:ff/00/00', '10:rgb:ff/ff/ff', 't:22']);
    handlers.dispose(); fallbacks.forEach(handler => handler.dispose()); term.dispose();
  });
});
