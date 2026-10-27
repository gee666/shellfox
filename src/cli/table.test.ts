import { expect, it } from 'vitest';
import { formatTable } from './table';
import { renderHuman } from './human';
import { printValue } from './output';
import { cliHelp } from './help';
import { styles, terminalWidth, textWidth, wrapText } from './terminal-text';
const id = '11111111-1111-4111-8111-111111111111';
const path = '/work/projects/very-long-project-name/with-more-folders';
const fits = (text: string, width: number) => {
  for (const line of text.split('\n')) expect(textWidth(line), JSON.stringify(line)).toBeLessThanOrEqual(width);
};
it('keeps aligned columns when the full table fits', () => {
  expect(formatTable(['NAME', 'STATE'], [['one', 'open'], ['longer', 'closed']], false, 80)).toBe('NAME    STATE\none     open\nlonger  closed');
});
it.each([20, 40, 60, 80, 100])('uses complete labeled records at %i columns without truncating data', width => {
  const text = formatTable(['NAME', 'ID', 'PATH'], [['project', id, path], ['other', id, path]], false, width);
  fits(text, width);
  expect(text.replaceAll('\n', '')).toContain(path); expect(text.replaceAll('\n', '')).toContain(id);
  expect(text).toContain('\n\n'); expect(text).not.toContain('…');
  if (width >= 40) expect(text).toContain(id);
});
it('measures color, combining marks, CJK and emoji in display cells', () => {
  expect(textWidth(styles(true).accent('界e\u0301👩‍💻'))).toBe(5);
  const value = styles(true).accent('界e\u0301👩‍💻'.repeat(20));
  fits(wrapText(value, 20), 20);
  const text = formatTable(['NAME', 'ID'], [['界e\u0301👩‍💻'.repeat(20), id]], true, 40);
  fits(text, 40); expect(text).toContain(id); expect(text).toContain('\x1b[');
});
it.each([20, 40, 60, 80, 100])('fits every human table shape and nested API data to %i columns', width => {
  const values: unknown[] = [
    [{ id, title: 'project', cwd: path, status: 'running', tabs: [1] }],
    [{ id, title: 'shell', sessionId: id, lifecycle: 'open', profileId: 'local:bash', cwd: path }],
    [{ id, hostname: 'workstation', online: true, connection: 'connected', approved: true, lastError: { message: path } }],
    [{ id, name: 'profile', executable: path, args: ['--login'], details: { directory: path } }],
    { sessions: [{ id, title: 'project', cwd: path, tabs: [1], status: 'running' }] },
  ];
  for (const value of values) {
    const text = renderHuman(value, true, width); fits(text, width);
    expect(text.replaceAll('\n', '').replaceAll(' ', '')).toContain(id);
  }
});
it('uses the destination width without changing JSON output', () => {
  let text = ''; const output = { columns: 40, write: (value: string) => { text += value; } };
  const value = [{ id, path }]; printValue(value, false, output); fits(text, 40); expect(text).toContain(id);
  text = ''; printValue(value, true, output); expect(JSON.parse(text)).toEqual({ ok: true, value });
  expect(terminalWidth({ columns: 40 })).toBe(40);
  for (const columns of [0, NaN, -1, Infinity]) expect(terminalWidth({ columns })).toBe(Infinity);
});
it.each([20, 40, 80])('fits root and alias help at %i columns', width => {
  for (const topic of [undefined, 'cmd', 'ss', 'tm']) fits(cliHelp('test', topic, true, width), width);
});
it('escapes untrusted API field names as well as values', () => {
  const text = renderHuman([{ ['bad\x1b[2J']: 'value\u202e' }], false, 40);
  fits(text, 40); expect(text).not.toMatch(/[\x1b\u202e]/); expect(text).toContain('\\u001b');
});
