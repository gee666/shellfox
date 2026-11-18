// Never invokes launch/serve/power-guardian/self-test. The only child workload
// is owned System32 cmd.exe with AutoRun disabled and an empty environment.
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import test from 'node:test';
import { helperEnvironment, projectRoot } from './dotnet-sdk.mjs';
import { auditNativePe } from './terminal-pe.mjs';

const directory = path.join(projectRoot, 'tmp/terminal-native-aot', `win-${process.arch}`);
const helper = path.join(directory, 'Shellfox.ElevatedTerminal.exe');
const managed = path.join(projectRoot, 'tmp/terminal-native', `win-${process.arch}`, 'Shellfox.ElevatedTerminal.exe');
const supported = process.platform === 'win32' && existsSync(helper);
function execute(file, args, input = '') {
  const result = spawnSync(file, args, { env: { ...process.env, ...helperEnvironment() }, input, encoding: 'utf8', timeout: 15000, windowsHide: true });
  assert.ifError(result.error); return result;
}
test('actual NativeAOT image and all three safe probes preserve managed bytes', { skip: !supported }, () => {
  const pe = auditNativePe(helper, process.arch);
  assert.equal(pe.dependentLoadFlags, '0x800'); assert.equal(pe.clrDirectory, 'absent');
  for (const mode of ['probe', 'probe-isolated', 'probe-power']) {
    const actual = execute(helper, [mode]); assert.equal(actual.status, 0); assert.equal(actual.stderr, '');
    if (existsSync(managed)) assert.equal(actual.stdout, execute(managed, [mode]).stdout);
    if (mode === 'probe') assert.equal(actual.stdout, 'shellfox-elevated-conpty-v1\r\n');
  }
  const record = JSON.parse(readFileSync(path.join(directory, 'native-aot-build.json'), 'utf8'));
  assert.equal(record.embeddedRuntimeVersion, '10.0.15'); assert.equal(record.compilerWarningsAreErrors, true);
  const audit = JSON.parse(readFileSync(path.join(directory, 'native-aot-nuget-audit.json'), 'utf8'));
  assert.equal(audit.status, 'no-listed-findings'); assert.equal(audit.runtimeLifecycle.status, 'current-in-listed-channel');
});
test('actual NativeAOT PipeRun preserves generic argv data after the narrow token fix', { skip: !supported, timeout: 20000 }, () => {
  mkdirSync(path.join(projectRoot, 'tmp'), { recursive: true });
  const scratch = mkdtempSync(path.join(projectRoot, 'tmp/native-aot-argv-test-'));
  try {
    const file = path.join(scratch, 'launch.json');
    const args = ['', 'plain', '--flag', 'spaces 雪🙂', 'quote"inside', 'slashes\\', 'before\\"after', '& | > < ^ %PATH% !x! (x)', '\\\\', 'tab\tline\nvalue'];
    const code = "let input='';process.stdin.setEncoding('utf8');process.stdin.on('data',x=>input+=x);process.stdin.on('end',()=>process.stdout.write(JSON.stringify({args:process.argv.slice(1),input})))";
    const systemRoot = process.env.SystemRoot || 'C:\\Windows';
    writeFileSync(file, JSON.stringify({ executable: process.execPath, args: ['-e', code, '--', ...args], cwd: scratch, env: { SystemRoot: systemRoot, WINDIR: systemRoot } }));
    const result = execute(helper, ['pipe-run', file], 'owned stdin 雪🙂\n');
    assert.equal(result.status, 0, result.stderr); assert.equal(result.stderr, '');
    assert.deepEqual(JSON.parse(result.stdout), { args, input: 'owned stdin 雪🙂\n' });
    assert.equal(existsSync(file), false);
  } finally { rmSync(scratch, { recursive: true, force: true }); }
});
test('actual NativeAOT PipeRun contains profile-free owned cmd and preserves raw output/exit', { skip: !supported, timeout: 20000 }, () => {
  mkdirSync(path.join(projectRoot, 'tmp'), { recursive: true });
  const scratch = mkdtempSync(path.join(projectRoot, 'tmp/native-aot-pipe-test-'));
  try {
    const file = path.join(scratch, 'launch.json');
    const executable = path.join(process.env.SystemRoot || 'C:\\Windows', 'System32/cmd.exe');
    writeFileSync(file, JSON.stringify({ executable, args: ['/d', '/q', '/c', 'echo SHELLFOX_AOT_PIPE_OK& exit /b 23'], cwd: scratch, env: {} }));
    const result = execute(helper, ['pipe-run', file]);
    assert.equal(result.status, 23, result.stderr); assert.equal(result.stdout, 'SHELLFOX_AOT_PIPE_OK\r\n'); assert.equal(result.stderr, '');
    assert.equal(existsSync(file), false);
  } finally { rmSync(scratch, { recursive: true, force: true }); }
});
