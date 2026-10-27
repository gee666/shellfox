import { afterEach, expect, it } from 'vitest';
import { execFile, spawnSync } from 'node:child_process';
import { promisify } from 'node:util';
import { mkdir, mkdtemp, readFile, rm, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { build } from 'esbuild';
import { bashCompletion, completionScript, dispatchCompletion, powershellCompletion, zshCompletion } from './shellfox-completion';
import { shellfoxShims } from './shellfox-cli';
import { linuxLauncher } from './linux-cli';

const exec = promisify(execFile);
const roots: string[] = [];
const fixture = async () => {
  await mkdir(path.resolve('tmp'), { recursive: true });
  const root = await mkdtemp(path.resolve('tmp/completion-')); roots.push(root); return root;
};
afterEach(async () => { await Promise.all(roots.splice(0).map(root => rm(root, { recursive: true, force: true }))); });
const bash = process.platform === 'win32' ? path.join(process.env.ProgramFiles ?? 'C:\\Program Files', 'Git/bin/bash.exe') : 'bash';
const available = (file: string) => !spawnSync(file, ['--version'], { timeout: 3000, windowsHide: true }).error;
const posix = (value: string) => value.replaceAll('\\', '/');
const psQuote = (value: string) => "'" + value.replaceAll("'", "''") + "'";
const ps = async (code: string) => (await exec('powershell.exe', ['-NoLogo', '-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass', '-EncodedCommand', Buffer.from(code, 'utf16le').toString('base64')], { timeout: 10000, maxBuffer: 128 * 1024 })).stdout;

it('prints only the requested hook and rejects unsupported shells or extra arguments', () => {
  for (const shell of ['powershell', 'bash', 'zsh']) {
    let out = '', error = '';
    const stdout = { write: (text: string) => { out += text; return true; } } as typeof process.stdout;
    const stderr = { write: (text: string) => { error += text; return true; } } as typeof process.stderr;
    expect(dispatchCompletion(['completion', shell], stdout, stderr)).toBe(0);
    expect(out).toBe(completionScript(shell)); expect(error).toBe('');
    expect(dispatchCompletion(['completion', shell, 'extra'], stdout, stderr)).toBe(2);
    expect(dispatchCompletion(['completion', 'fish'], stdout, stderr)).toBe(2);
    expect(dispatchCompletion(['cmd'], stdout, stderr)).toBeNull();
  }
  expect(completionScript('fish')).toBeNull();
  for (const hook of [powershellCompletion(), bashCompletion(), zshCompletion()]) {
    expect(hook).not.toContain('cdm'); expect(hook).not.toContain('archive');
    expect(hook).toContain('settle'); expect(hook).toContain('unsettle'); expect(hook).toContain('delete');
    expect(hook).not.toMatch(/Set-PSReadLineKeyHandler|bindkey|Ctrl\+Space/);
  }
});

it.skipIf(!available(bash))('Bash completes names and flags only at custom-command positions, treats endpoint output as data', async () => {
  const root = await fixture(), hook = path.join(root, 'hook.bash');
  await writeFile(hook, bashCompletion());
  const script = `source "$1"
source "$1"
log="$2"
shellfox() {
  printf '%s|%s|%s\\n' "$1" "$2" "$3" >> "$log"
  [[ "$1" == cmd && "$2" == --complete ]] || return 99
  [[ "$3" != fail ]] || { printf 'Deploy\\n'; return 1; }
  printf 'Deploy\\r\\ndevelopment\\n$(touch unsafe)\\n--evil\\n'
}
check() {
  COMP_CWORD=$1; shift; COMP_WORDS=("$@")
  _shellfox_complete
  printf '[%s]\\n' "\${COMPREPLY[*]}"
}
check 1 shellfox c
check 2 shellfox cmd --r
check 2 shellfox cmd ''
check 3 shellfox cmd --run De
check 3 shellfox cmd --remove De
check 3 shellfox cmd --rm De
check 3 shellfox cmd --unregister De
check 3 shellfox cmd --rename De
check 3 shellfox cmd --register new
check 4 shellfox cmd --rename old new
check 3 shellfox cmd Deploy arg
check 4 shellfox cmd --run Deploy arg
check 2 shellfox ls ''
check 2 shellfox cmd fail
check 1 shellfox se
check 1 shellfox un
check 2 shellfox settle ''
check 2 shellfox unsettle ''
`;
  const log = path.join(root, 'queries');
  const { stdout } = await exec(bash, ['--noprofile', '--norc', '-c', script, 'completion-test', posix(hook), posix(log)], { cwd: root, timeout: 10000 });
  const lines = stdout.trim().split(/\r?\n/);
  expect(lines[0]).toBe('[commands completion]');
  expect(lines[1]).toBe('[--run --register --remove --rm --rename]');
  expect(lines[2]).toContain('Deploy development');
  expect(lines.slice(3, 8)).toEqual(Array(5).fill('[Deploy development]'));
  expect(lines.slice(8, 14)).toEqual(Array(6).fill('[]'));
  expect(lines.slice(14)).toEqual(['[session settings]', '[]', '[]', '[]']);
  const queries = await readFile(log, 'utf8');
  expect(queries.trim().split(/\r?\n/)).toHaveLength(7);
  expect(queries).toContain('cmd|--complete|De');
  expect(queries).not.toContain('arg');
});

it.skipIf(process.platform !== 'win32')('PowerShell TabExpansion2 shows hints, preserves the prefix and does not complete script arguments', async () => {
  const root = await fixture(), hook = path.join(root, 'hook.ps1'), shim = path.join(root, 'shellfox.ps1'), log = path.join(root, 'queries');
  await writeFile(hook, powershellCompletion());
  await writeFile(shim, `@($args) | ConvertTo-Json -Compress | Add-Content -LiteralPath ${psQuote(log)}
if ($args[2] -eq 'fail') { exit 1 }
'Deploy'; 'development'; '$(touch unsafe)'; '--evil'
exit 0
`);
  const result = await ps(`$ErrorActionPreference='Stop'
$env:PATH=${psQuote(root + ';')}+$env:PATH
. ${psQuote(hook)}
. ${psQuote(hook)}
$global:LASTEXITCODE=42
$lines=@('shellfox comm','shellfox cmd --r','shellfox cmd ','shellfox cmd --run De','shellfox cmd --remove De','shellfox cmd --rm De','shellfox cmd --unregister De','shellfox cmd --rename De','shellfox cmd --register new','shellfox cmd --rename old new','shellfox cmd Deploy arg','shellfox cmd --run Deploy arg','shellfox cmd fail','shellfox se','shellfox un','shellfox de')
$results=@(foreach ($line in $lines) {
  $matches=@((TabExpansion2 $line $line.Length).CompletionMatches | Where-Object { $_.CompletionText -match '^(commands|--|Deploy|development|session|settings|settle|unsettle|delete)'  } | ForEach-Object { @{text=$_.CompletionText;hint=$_.ToolTip} })
  ,$matches
})
@{results=$results;last=$global:LASTEXITCODE} | ConvertTo-Json -Depth 8 -Compress
`);
  const { results, last } = JSON.parse(result);
  expect(results[0].map((item: { text: string }) => item.text)).toEqual(['commands']);
  expect(results[0][0].hint).toContain('script');
  expect(results[1].some((item: { text: string; hint: string }) => item.text === '--rename' && item.hint.includes('Rename'))).toBe(true);
  expect(results[2].some((item: { text: string }) => item.text === 'Deploy')).toBe(true);
  for (const matches of results.slice(3, 8)) expect(matches.map((item: { text: string }) => item.text)).toEqual(['Deploy', 'development']);
  expect(results.slice(8, 13)).toEqual(Array(5).fill([]));
  expect(results.slice(13).map((matches: { text: string }[]) => matches.map(item => item.text))).toEqual([['session', 'settings'], [], []]);
  expect(last).toBe(42);
  const queries = (await readFile(log, 'utf8')).trim().split(/\r?\n/).map(line => JSON.parse(line));
  expect(queries).toHaveLength(7);
  expect(queries[0]).toEqual(['cmd', '--complete', '']);
  expect(queries[1]).toEqual(['cmd', '--complete', 'De']);
});

it.skipIf(!available('zsh'))('zsh hook parses and supplies descriptions and names without evaluating registry data', async () => {
  const root = await fixture(), hook = path.join(root, 'hook.zsh');
  await writeFile(hook, zshCompletion());
  await exec('zsh', ['-n', hook], { timeout: 5000 });
  const { stdout } = await exec('zsh', ['-f', '-c', `compdef() { :; }
_describe() { print -r -- "$1"; print -rl -- "\${(@P)2}"; }
compadd() { print -rl -- "$@"; }
shellfox() { [[ $1 == cmd && $2 == --complete ]] || return 1; print -rl -- Deploy development '$(touch unsafe)'; }
source "$1"
words=(shellfox cmd ''); CURRENT=3; PREFIX=''; _shellfox_complete
words=(shellfox cmd --rename De); CURRENT=4; PREFIX=De; _shellfox_complete
words=(shellfox cmd --run Deploy argument); CURRENT=5; PREFIX=argument; _shellfox_complete
`, 'completion-test', hook], { cwd: root, timeout: 5000 });
  expect(stdout).toContain('--rename:Rename a registered command');
  expect(stdout).toContain('Registered command'); expect(stdout).toContain('Deploy');
  expect(stdout).not.toContain('touch unsafe');
});

it('routes completion generation through the bundled helper without reading or creating user data', async () => {
  const root = await fixture(), helper = path.join(root, 'main/cli.cjs'), userData = path.join(root, 'absent-data');
  await build({ entryPoints: ['src/main/platform/shellfox-cli-entry.ts'], outfile: helper, bundle: true, platform: 'node', format: 'cjs', packages: 'external', loader: { '.sh': 'text' }, define: { __SHELLFOX_VERSION__: '"test"' } });
  for (const shell of ['powershell', 'bash', 'zsh']) {
    const { stdout, stderr } = await exec(process.execPath, [helper, '--user-data', userData, 'completion', shell], { timeout: 10000 });
    expect(stdout).toBe(completionScript(shell)); expect(stderr).toBe('');
  }
  await expect(readFile(userData)).rejects.toMatchObject({ code: 'ENOENT' });
  await expect(exec(process.execPath, [helper, 'completion', 'fish'], { timeout: 10000 })).rejects.toMatchObject({ code: 2 });
  if (process.platform === 'win32') {
    const shims = shellfoxShims({ executable: process.execPath, cliHelper: helper, userData });
    const runtime = path.win32.join(path.dirname(helper), '../cli-runtime/node.exe');
    const powershell = path.join(root, 'shellfox.ps1'), cmd = path.join(root, 'shellfox.cmd');
    await writeFile(powershell, shims.powershell.replaceAll(runtime, process.execPath));
    await writeFile(cmd, shims.cmd.replaceAll(runtime, process.execPath));
    expect((await ps(`& ${psQuote(powershell)} completion powershell`)).replaceAll('\r\n', '\n')).toBe(powershellCompletion());
    const { stdout } = await exec('cmd.exe', ['/d', '/c', cmd, 'completion', 'bash'], { timeout: 10000 });
    expect(stdout).toBe(bashCompletion());
  }
  // Both POSIX launchers route non-GUI commands through the same helper.
  expect(shellfoxShims({ executable: 'C:\\app.exe' }).posix).toContain('exec "$runtime" "$helper"');
  expect(linuxLauncher({ executable: '/app' })).toContain('cli/main/cli.cjs');
});
