import { expect, it } from 'vitest';
import { colorEnabled, humanTable, renderHuman } from './human';
import { textWidth, wrapText } from './terminal-text';
import { printValue } from './output';
import { cliHelp } from './help';
it('renders human-readable key values and tables by default, never a JSON dump', () => {
  let text = ''; printValue({ mode: 'headless', pid: 42, capabilities: { administrator: false } }, false, { write: value => { text += value; } });
  expect(text).toContain('mode'); expect(text).toContain('headless'); expect(text).toContain('administrator  no');
  expect(text).not.toContain('{'); expect(text).not.toContain('"mode"');
  expect(renderHuman([{ name: 'LANG', value: 'C' }])).toContain('NAME');
});
it('uses colored headings and status only on a TTY, respecting NO_COLOR and TERM', () => {
  expect(colorEnabled({ isTTY: true }, {})).toBe(true);
  expect(colorEnabled({ isTTY: true }, { NO_COLOR: '' })).toBe(false);
  expect(colorEnabled({ isTTY: false }, { FORCE_COLOR: '1' })).toBe(false);
  expect(colorEnabled({ isTTY: true }, { TERM: 'dumb' })).toBe(false);
  expect(renderHuman({ status: 'online' }, true)).toContain('\x1b[');
  expect(renderHuman({ status: 'online' }, false)).not.toContain('\x1b[');
});
it('prints full session and terminal identities in concise tables', () => {
  const id = '11111111-1111-4111-8111-111111111111';
  const text = renderHuman([{ id, title: 'project', cwd: '/work', status: 'running', tabs: [1] }]);
  expect(text).toContain(id); expect(text).toContain('TERMINALS'); expect(text).toContain('running');
});
it('keeps nested API data readable instead of dropping non-scalar fields', () => {
  const text = renderHuman([{ id: 'bash', label: 'Bash', args: ['-l'], environment: 'local' }]);
  expect(text).toContain('ARGS'.toLowerCase()); expect(text).toContain('-l'); expect(text).not.toContain('["');
});
it('escapes terminal controls in human strings, field values and table data', () => {
  const text = renderHuman([{ name: 'bad\x1b[2J', value: '\x9b31m\u202e' }]);
  expect(text).not.toMatch(/[\x1b\x9b\u202e]/); expect(text).toContain('\\u001b');
});
it('has one canonical Session section and no top-level session or tab commands', () => {
  const text = cliHelp('test'); expect(text.match(/^Session$/gm)).toHaveLength(1);
  for (const name of ['start', 'add', 'new', 'create', 'ls', 'stats', 'processes', 'settle', 'delete', 'tab']) expect(text).not.toMatch(new RegExp('^  ' + name + '(?: |$)', 'm'));
  expect(text).toContain('session delete'); expect(text).toContain('terminals close'); expect(text).not.toContain('Host and automation');
  expect(cliHelp('test', 'ss')).toContain('Explicit'.toLowerCase());
  for (const alias of ['ss', 'sessions', 'tm', 'cmd', '--run', '--ls', '--rm', '--unregister', '--version', '-v', '--help', '-h']) expect(text).toContain(alias);
  expect(cliHelp('test', 'cmd')).toBe(cliHelp('test', 'commands'));
  expect(cliHelp('test', 'terminals')).toContain('terminals attach <tab-id>');
  expect(cliHelp('test', 'terminals')).not.toContain('--session-id');
});
