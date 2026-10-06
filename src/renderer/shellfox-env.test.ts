import { describe, expect, it } from 'vitest';
import { formatShellfoxEnv, parseShellfoxEnv } from './shellfox-env';

describe('Shellfox .env editor syntax', () => {
  it('ignores comments and blanks, handles export and quotes, and keeps = inside values', () => {
    expect(parseShellfoxEnv('# Comment\r\n\r\nexport API_KEY = "sample=key"\r\nNODE_ENV=development\nEMPTY=\nSINGLE=\' spaces \'\nHASH=literal # text', true)).toEqual({ env: [
      { name: 'API_KEY', value: 'sample=key' }, { name: 'NODE_ENV', value: 'development' },
      { name: 'EMPTY', value: '' }, { name: 'SINGLE', value: ' spaces ' }, { name: 'HASH', value: 'literal # text' },
    ], issues: [] });
  });
  it('reports line numbers for invalid syntax, empty names, and unclosed quotes', () => {
    const result = parseShellfoxEnv('# Skip\nBROKEN\n =value\nA="unfinished', true);
    expect(result.issues.map(issue => issue.line)).toEqual([2, 3, 4]);
    expect(result.issues.map(issue => issue.message)).toEqual(['Use NAME=value.', 'Name must be 1 to 256 characters with no whitespace, =, :, / or NUL.', 'Values must be single-line; close the quoted value.']);
  });
  it('rejects Windows case-insensitive duplicates but permits distinct case on other platforms', () => {
    expect(parseShellfoxEnv(' PATH =one\npath=two', true).issues).toEqual([{ line: 2, message: 'Duplicate variable name.' }]);
    expect(parseShellfoxEnv('PATH=one\npath=two', false).issues).toEqual([]);
  });
  it('enforces name, value, NUL and entry limits', () => {
    expect(parseShellfoxEnv(`${'A'.repeat(256)}=${'x'.repeat(32767)}`, true).issues).toEqual([]);
    expect(parseShellfoxEnv(`${'A'.repeat(257)}=value`, true).issues[0]?.line).toBe(1);
    expect(parseShellfoxEnv(`A=${'x'.repeat(32768)}`, true).issues[0]?.message).toContain('32767');
    expect(parseShellfoxEnv('A\0=value\nB=a\0b', true).issues).toHaveLength(2);
    const maximum = Array.from({ length: 200 }, (_, index) => `KEY_${index}=value`).join('\n');
    expect(parseShellfoxEnv(maximum, true).env).toHaveLength(200);
    expect(parseShellfoxEnv(`${maximum}\nEXTRA=value`, true).issues).toEqual([{ line: 201, message: 'Use at most 200 variables.' }]);
  });
  it('rejects WSL delimiters and whitespace names with clear line errors', () => { for (const name of ['BAD:NAME', 'BAD/NAME', 'BAD NAME', 'BAD\tNAME']) { expect(parseShellfoxEnv(`${name}=value`, true).issues).toEqual([{ line: 1, message: 'Name must be 1 to 256 characters with no whitespace, =, :, / or NUL.' }]); } });
  it('never silently turns persisted multiline values into additional variables', () => { for (const value of ['first\nOTHER=value', 'first\rOTHER=value', 'first\r\nOTHER=value', 'tail\r']) { const result = parseShellfoxEnv(formatShellfoxEnv([{ name: 'OLD', value }]), true); expect(result.issues.length).toBeGreaterThan(0); } });
  it('round-trips values without interpreting escapes or interpolating variables', () => {
    const env = [
      { name: 'EMPTY', value: '' }, { name: 'SPACES', value: '  value  ' },
      { name: 'QUOTES', value: '"double" and \'single\'' }, { name: 'BACKSLASH', value: 'C:\\tools\\$HOME' },
      { name: 'EQUALS', value: 'a=b=c' }, { name: 'LITERAL', value: '${HOME} %PATH% # literal' },
    ];
    expect(parseShellfoxEnv(formatShellfoxEnv(env), true)).toEqual({ env, issues: [] });
    expect(formatShellfoxEnv([{ name: 'NODE_ENV', value: 'development' }])).toBe('NODE_ENV=development');
  });
});
