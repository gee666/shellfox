import path from 'node:path';
import { spawn, execFile } from 'node:child_process';
import { promisify } from 'node:util';
import { access } from 'node:fs/promises';
import { requireScriptFile, type ScriptLocation } from './shellfox-cli-commands';

const exec = promisify(execFile);
export interface ScriptContext {
  platform: NodeJS.Platform;
  cwd: string;
  env: NodeJS.ProcessEnv;
}
export interface ScriptInvocation { executable: string; args: string[]; cwd: string; env: NodeJS.ProcessEnv; msys?: boolean }
// MSYS treats apostrophes as quoting even when libuv would leave an argument bare.
// Quote every argument, including empty strings, rather than only ones with spaces.
export const quoteMsysArgument = (value: string): string => '"' + value.replace(/(\\*)"/g, '$1$1\\"').replace(/(\\+)$/, '$1$1') + '"';
export const scriptContext = (): ScriptContext => ({ platform: process.platform, cwd: process.cwd(), env: process.env });
const wslExecutable = (context: ScriptContext) => path.win32.join(context.env.SystemRoot ?? 'C:\\Windows', 'System32', 'wsl.exe');
const wslDistro = (context: ScriptContext) => context.platform === 'win32' ? context.env.SHELLFOX_CLI_DISTRO : undefined;
const checkText = (text: string) => { if (/[\0\r\n]/.test(text)) throw new Error('Script paths cannot contain NUL or line breaks.'); };
async function capture(executable: string, args: string[], context: ScriptContext, msys = false): Promise<string> {
  if (!msys) return (await exec(executable, args, { cwd: context.cwd, env: context.env, timeout: 10000, maxBuffer: 65536 })).stdout.trim();
  return new Promise((resolve, reject) => {
    const child = spawn(executable, args.map(quoteMsysArgument), { cwd: context.cwd, env: context.env, timeout: 10000,
      windowsVerbatimArguments: true, argv0: quoteMsysArgument(executable), stdio: ['ignore', 'pipe', 'pipe'] });
    let stdout = '', stderr = '';
    const collect = (chunk: Buffer, error: boolean) => {
      if (error) stderr += chunk.toString(); else stdout += chunk.toString();
      if (stdout.length + stderr.length > 65536) { child.kill(); reject(new Error('Path conversion output exceeded its limit.')); }
    };
    child.stdout.on('data', chunk => collect(chunk, false)); child.stderr.on('data', chunk => collect(chunk, true));
    child.once('error', reject);
    child.once('close', code => code === 0 ? resolve(stdout.trim()) : reject(new Error(stderr || 'Path conversion failed.')));
  });
}
export async function findGitBash(context: ScriptContext): Promise<string> {
  const supplied = context.env.SHELLFOX_CLI_GIT_BASH;
  if (supplied) {
    checkText(supplied);
    if (!path.win32.isAbsolute(supplied)) throw new Error('Git Bash interpreter must be an absolute path.');
    await access(supplied); return supplied;
  }
  const candidates = [context.env.ProgramFiles, context.env['ProgramFiles(x86)']].filter(Boolean)
    .map(root => path.win32.join(root!, 'Git', 'bin', 'bash.exe'));
  if (context.env.LOCALAPPDATA) candidates.push(path.win32.join(context.env.LOCALAPPDATA, 'Programs', 'Git', 'bin', 'bash.exe'));
  for (const candidate of candidates) { try { await access(candidate); return candidate; } catch { /* Try next installation. */ } }
  throw new Error('Running .sh scripts on Windows requires Git Bash. Install Git, run from Git Bash, or register the script inside WSL.');
}
export async function resolveScriptLocation(input: string, context = scriptContext()): Promise<ScriptLocation> {
  checkText(input);
  if (!input) throw new Error('Missing script path.');
  const distro = wslDistro(context);
  if (distro) {
    const cwd = context.env.SHELLFOX_CLI_WSL_CWD;
    if (!cwd || !path.posix.isAbsolute(cwd)) throw new Error('Missing WSL caller directory. Run shellfox from its WSL shim.');
    // Windows paths are explicitly converted by WSL, not guessed as guest paths.
    const guest = path.win32.isAbsolute(input) && !path.posix.isAbsolute(input)
      ? await capture(wslExecutable(context), ['--distribution', distro, '--exec', 'wslpath', '-u', input], context)
      : path.posix.resolve(cwd, input);
    await capture(wslExecutable(context), ['--distribution', distro, '--exec', '/usr/bin/test', '-f', guest], context);
    return { path: guest, distro };
  }
  let file: string;
  if (context.platform === 'win32' && context.env.SHELLFOX_CLI_GIT_BASH && !/^[a-z]:[\\/]|^\\\\/i.test(input)) {
    const bash = await findGitBash(context);
    const cygpath = path.win32.join(path.win32.dirname(bash), 'cygpath.exe');
    file = await capture(cygpath, ['-wa', '--', input], { ...context, cwd: context.env.SHELLFOX_CLI_GIT_CWD || context.cwd }, true);
  } else {
    file = (context.platform === 'win32' ? path.win32 : path.posix).resolve(context.cwd, input);
  }
  await requireScriptFile(file);
  if (context.platform === 'win32' && /\.(bat|cmd)$/i.test(file)) throw new Error('Batch scripts are not supported. Use .ps1 or .sh to preserve literal arguments.');
  return { path: file };
}

export async function scriptInvocation(location: ScriptLocation, args: string[], context = scriptContext()): Promise<ScriptInvocation> {
  if (args.some(arg => arg.includes('\0'))) throw new Error('Arguments cannot contain NUL.');
  checkText(location.path);
  // Electron's Node switch must not leak into user scripts or programs they launch.
  const env = { ...context.env }; delete env.ELECTRON_RUN_AS_NODE;
  const cwd = context.platform === 'win32' && context.env.SHELLFOX_CLI_GIT_CWD ? context.env.SHELLFOX_CLI_GIT_CWD : context.cwd;
  const extension = path.extname(location.path).toLowerCase();
  if (location.distro) {
    if (context.platform !== 'win32') throw new Error('This command belongs to WSL. Run it with Shellfox on Windows.');
    const guestCwd = wslDistro(context) === location.distro && context.env.SHELLFOX_CLI_WSL_CWD
      ? context.env.SHELLFOX_CLI_WSL_CWD
      : await capture(wslExecutable(context), ['--distribution', location.distro, '--exec', 'wslpath', '-u', cwd], context);
    // --exec bypasses the user's login shell. Nothing here is shell command text.
    const command = extension === '.sh' ? ['/bin/bash', '--', location.path, ...args] : [location.path, ...args];
    return { executable: wslExecutable(context), args: ['--distribution', location.distro, '--cd', guestCwd, '--exec', ...command], cwd, env };
  }
  if (extension === '.sh') {
    const executable = context.platform === 'win32' ? await findGitBash(context) : 'bash';
    let file = location.path;
    if (context.platform === 'win32') {
      // Forward slashes work in Git Bash and preserve spaces without bash -c.
      file = file.replaceAll('\\', '/'); env.MSYS_ARG_CONV_EXCL = '*';
    }
    return { executable, args: ['--', file, ...args], cwd, env, ...(context.platform === 'win32' ? { msys: true } : {}) };
  }
  if (extension === '.ps1') {
    const payload = Buffer.from(JSON.stringify({ path: location.path, args }), 'utf8').toString('base64');
    const code = `$ErrorActionPreference='Stop'; $ProgressPreference='SilentlyContinue'; $global:LASTEXITCODE=0; try { $p=[Text.Encoding]::UTF8.GetString([Convert]::FromBase64String('${payload}')) | ConvertFrom-Json; $scriptArgs=@($p.args); & $p.path @scriptArgs; $scriptOk=$?; if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }; if (-not $scriptOk) { exit 1 }; exit 0 } catch { [Console]::Error.WriteLine($_.Exception.Message); exit 1 }`;
    return { executable: context.platform === 'win32' ? path.win32.join(context.env.SystemRoot ?? 'C:\\Windows', 'System32', 'WindowsPowerShell', 'v1.0', 'powershell.exe') : 'pwsh',
      args: ['-NoLogo', '-NoProfile', '-ExecutionPolicy', 'Bypass', '-EncodedCommand', Buffer.from(code, 'utf16le').toString('base64')], cwd, env };
  }
  if (context.platform === 'win32' && extension !== '.exe' && extension !== '.com') throw new Error('Windows custom scripts must use .sh or .ps1, or be .exe/.com programs. Batch scripts are not supported.');
  return { executable: location.path, args, cwd, env };
}

export async function executeScript(location: ScriptLocation, args: string[], context = scriptContext()): Promise<number> {
  const invocation = await scriptInvocation(location, args, context);
  return new Promise((resolve, reject) => {
    const argv = invocation.msys ? invocation.args.map(quoteMsysArgument) : invocation.args;
    const child = spawn(invocation.executable, argv, { cwd: invocation.cwd, env: invocation.env, stdio: 'inherit', shell: false, windowsVerbatimArguments: invocation.msys, argv0: invocation.msys ? quoteMsysArgument(invocation.executable) : undefined });
    // Keep the helper alive on Ctrl+C until the foreground child closes.
    const interrupted = () => { if (context.platform === 'win32') child.kill('SIGINT'); };
    process.on('SIGINT', interrupted);
    child.once('error', error => { process.off('SIGINT', interrupted); reject(error); });
    child.once('close', (code, signal) => { process.off('SIGINT', interrupted); resolve(code ?? (signal === 'SIGINT' ? 130 : signal === 'SIGTERM' ? 143 : 1)); });
  });
}

/** Called only by the Windows POSIX shim, before it launches Windows Node. */
export function customScriptContext(helper: string): string {
  const prefix = `if [ -n "\${WSL_DISTRO_NAME:-}" ]; then
  export SHELLFOX_CLI_DISTRO="$WSL_DISTRO_NAME"
  export SHELLFOX_CLI_WSL_CWD="$PWD"
elif command -v cygpath >/dev/null 2>&1; then
  shellfox_bash=$(cygpath -w "$(command -v bash)") || exit 1
  case "$shellfox_bash" in *.exe) ;; *) shellfox_bash=$shellfox_bash.exe ;; esac
  export SHELLFOX_CLI_GIT_BASH="$shellfox_bash"
  export SHELLFOX_CLI_GIT_CWD="$(cygpath -w "$PWD")"
fi
`;
  return prefix + helper.replace('if [ "${NO_COLOR+x}" = x ]; then', 'shellfox_wsl_env SHELLFOX_CLI_DISTRO\n  shellfox_wsl_env SHELLFOX_CLI_WSL_CWD\n  if [ "${NO_COLOR+x}" = x ]; then');
}
