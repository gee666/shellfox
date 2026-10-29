import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { afterEach, describe, expect, it } from 'vitest';
import { resolveAutomationExecutable } from './automation-executable';

const directories: string[] = [];
afterEach(() => { for (const directory of directories.splice(0)) rmSync(directory, { recursive: true, force: true }); });
function fixture() {
  mkdirSync('tmp', { recursive: true });
  const directory = mkdtempSync(path.resolve('tmp/automation-path-')); directories.push(directory);
  return directory;
}
describe.skipIf(process.platform !== 'win32')('Windows automation executable resolution', () => {
  it('uses caller cwd, case-insensitive Path and PATHEXT without shell parsing', () => {
    const cwd = fixture(), bin = path.join(cwd, 'bin with spaces'); mkdirSync(bin);
    const executable = path.join(bin, 'literal & name.exe'); writeFileSync(executable, 'fixture');
    expect(resolveAutomationExecutable('literal & name', cwd, { Path: `"${bin}"`, Pathext: '.COM;.EXE;.BAT;.CMD' })).toBe(executable);
    expect(resolveAutomationExecutable('bin with spaces/literal & name.exe', cwd, {})).toBe(executable);
  });
  it('rejects implicit batch execution and missing programs', () => {
    const cwd = fixture(); writeFileSync(path.join(cwd, 'tool.cmd'), '@exit /b 0');
    expect(() => resolveAutomationExecutable('tool', cwd, { PATH: cwd, PATHEXT: '.CMD;.EXE' })).toThrow('cmd.exe');
    expect(() => resolveAutomationExecutable('tool.cmd', cwd, {})).toThrow('cmd.exe');
    expect(() => resolveAutomationExecutable('missing.exe', cwd, {})).toThrow('not found');
  });
});
