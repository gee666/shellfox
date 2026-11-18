import { describe, expect, it } from 'vitest';
import { parseGlobals } from './arguments';
import { parseAutomationCommand } from './automation';

describe('automation command', () => {
  it('protects literal program flags and values from global parsing', () => {
    const parsed = parseGlobals(['--json', 'run', '--title', '--yes', '--detach', '--', 'node', '--json', '--user-data', 'literal']);
    expect(parsed.options).toMatchObject({ json: true, yes: false });
    expect(parseAutomationCommand(parsed.args)).toEqual({ kind: 'run', executable: 'node', args: ['--json', '--user-data', 'literal'], cwd: undefined, title: '--yes', detached: true });
  });
  it('accepts a caller-known run ID without treating program arguments as options', () => {
    const id = '11111111-1111-4111-8111-111111111111';
    expect(parseAutomationCommand(['run', '--request-id', id, '--', 'node', '--request-id', 'literal'])).toMatchObject({ requestId: id, args: ['--request-id', 'literal'] });
    expect(() => parseAutomationCommand(['run', '--request-id', 'bad', '--', 'node'])).toThrow('UUID');
  });
});
