import { spawn } from 'node:child_process';
import path from 'node:path';
import { stripVTControlCharacters } from 'node:util';
import { z } from 'zod';

const root = path.resolve('.');
export const cliTtyEntry = path.join(root, 'tmp/build-test/cli/main/cli.cjs');
export const cliTtyRuntime = process.platform === 'win32' ? path.join(root, 'tmp/build-test/cli/cli-runtime/node.exe') : process.execPath;
const driverPath = path.join(root, 'tests/fixtures/cli-tty-driver.cjs');
const driverEvent = z.discriminatedUnion('type', [
  z.object({ type: z.literal('ready'), pid: z.number().int().positive(), runtime: z.string(), module: z.string() }).strict(),
  z.object({ type: z.literal('data'), data: z.string().max(1024 * 1024) }).strict(),
  z.object({ type: z.literal('exit'), exitCode: z.number().int(), signal: z.number().int().optional() }).strict(),
  z.object({ type: z.literal('ack'), id: z.number().int().positive(), method: z.enum(['write', 'kill']) }).strict(),
  z.object({ type: z.literal('error'), message: z.string().max(65536) }).strict(),
]);
type DriverEvent = z.infer<typeof driverEvent>;
function isolatedEnv(): NodeJS.ProcessEnv {
  const environment: NodeJS.ProcessEnv = { ...process.env, NO_COLOR: '1', TERM: 'xterm-256color', TEMP: path.join(root, 'tmp'), TMP: path.join(root, 'tmp') };
  for (const key of ['FORCE_COLOR', 'NODE_OPTIONS', 'NODE_PATH', 'SHELLFOX_TERMINAL_MARKER', 'ELECTRON_RUN_AS_NODE', 'SHELLFOX_USER_DATA', 'SHELLFOX_HEADLESS', 'SHELLFOX_CLI_DISTRO', 'SHELLFOX_CLI_WSL_CWD', 'SHELLFOX_CLI_GIT_BASH', 'SHELLFOX_CLI_GIT_CWD', 'SHELLFOX_RESOURCES_PATH', 'SHELLFOX_RUNTIME_PATH', 'SHELLFOX_PROJECT_ROOT', 'SHELLFOX_CLI_LAUNCH', 'SHELLFOX_CLI_UPDATE']) delete environment[key];
  return environment;
}

/** JSON-lines over ordinary stdio pipes. No native addon or IPC in the test worker. */
export function externalTty(userData: string, args: string[], cwd: string) {
  const config = Buffer.from(JSON.stringify({ userData, args, cwd })).toString('base64');
  const child = spawn(cliTtyRuntime, [driverPath, config], { cwd: root, env: isolatedEnv(), stdio: ['pipe', 'pipe', 'pipe'], windowsHide: true, shell: false });
  const events: DriverEvent[] = [], errors: string[] = [];
  const commands: Array<{ id: number; method: 'write' | 'kill'; data?: string }> = [];
  let raw = '', buffered = '', stderr = '', nextId = 0, exitCode: number | undefined;
  let driverExit: { code: number | null; signal: NodeJS.Signals | null } | undefined;
  let closed = false;
  const completed = new Promise<void>(resolve => {
    child.once('error', error => { errors.push('Driver spawn failed: ' + error.message); });
    child.once('close', (code, signal) => { closed = true; driverExit = { code, signal }; resolve(); });
  });
  function send(method: 'write' | 'kill', data?: string) {
    if (closed || child.stdin.destroyed) throw new Error('PTY driver is closed.');
    const command = { id: ++nextId, method, ...(data === undefined ? {} : { data }) }; commands.push(command);
    child.stdin.write(JSON.stringify(command) + '\n', error => { if (error) errors.push('Driver command failed: ' + error.message); });
  }
  child.stdin.on('error', error => { errors.push('Driver input failed: ' + error.message); });
  child.stderr.setEncoding('utf8'); child.stderr.on('data', (text: string) => { stderr += text; });
  child.stdout.setEncoding('utf8'); child.stdout.on('data', (chunk: string) => {
    buffered += chunk;
    if (Buffer.byteLength(buffered) > 2 * 1024 * 1024) { errors.push('PTY driver emitted an oversized JSON line.'); buffered = ''; return; }
    let newline: number;
    while ((newline = buffered.indexOf('\n')) >= 0) {
      const line = buffered.slice(0, newline); buffered = buffered.slice(newline + 1);
      try {
        const event = driverEvent.parse(JSON.parse(line)); events.push(event);
        if (event.type === 'data') {
          raw += event.data;
          if (Buffer.byteLength(raw) > 8 * 1024 * 1024) { errors.push('PTY output exceeded the evidence limit.'); raw = raw.slice(-4 * 1024 * 1024); send('kill'); }
        } else if (event.type === 'exit') {
          if (exitCode !== undefined) errors.push('PTY driver reported duplicate exit events.');
          exitCode = event.exitCode;
        } else if (event.type === 'error') errors.push(event.message);
      } catch (error) { errors.push('Invalid driver JSON ' + JSON.stringify(line.slice(0, 4096)) + ': ' + (error instanceof Error ? error.message : String(error))); }
    }
  });
  function assertHealthy() {
    if (errors.length || stderr.trim() || driverExit && (driverExit.code !== 0 || exitCode === undefined)) {
      throw new Error('External PTY driver failed: ' + JSON.stringify({ errors, stderr, driverExit, terminalExitCode: exitCode, recentEvents: events.slice(-8) }));
    }
  }
  const evidence = () => ({ driver: driverPath, runtime: cliTtyRuntime, userData, args, cwd, driverPid: child.pid, driverExit, terminalExitCode: exitCode, errors: [...errors], stderr, events: [...events], commands: [...commands], output: stripVTControlCharacters(raw).replaceAll('\r', '') });
  async function waitForClose(ms: number): Promise<void> {
    if (closed) return;
    let timer: ReturnType<typeof setTimeout> | undefined;
    try { await Promise.race([completed, new Promise<void>(resolve => { timer = setTimeout(resolve, ms); })]); }
    finally { if (timer) clearTimeout(timer); }
  }
  let cleanup: Promise<void> | undefined;
  function close(): Promise<void> {
    return cleanup ??= (async () => {
      if (!closed && exitCode === undefined) send('write', '\x1d');
      await waitForClose(5000);
      if (!closed) { send('kill'); await waitForClose(7000); }
      if (!closed) { child.kill('SIGTERM'); await waitForClose(6000); }
      if (!closed) { errors.push('Driver required forced termination after owned PTY cleanup failed.'); child.kill('SIGKILL'); await completed; }
      if (buffered.trim()) errors.push('PTY driver closed with an incomplete JSON event.');
      assertHealthy();
    })();
  }
  return {
    terminal: { write(data: string) { assertHealthy(); send('write', data); }, kill() { send('kill'); } },
    text: () => { assertHealthy(); return stripVTControlCharacters(raw).replaceAll('\r', ''); },
    raw: () => { assertHealthy(); return raw; },
    code: () => { assertHealthy(); return exitCode; },
    evidence, close,
  };
}
