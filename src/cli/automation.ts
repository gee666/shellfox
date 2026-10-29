import { randomUUID } from 'node:crypto';
import type { Readable, Writable } from 'node:stream';
import { setTimeout as delay } from 'node:timers/promises';
import type { HostClient } from '../host/client';
import type { HostResponse } from '../shared/host-contracts';
import { automationRequestSchemas, automationResponseSchemas, automationRunSchema, type AutomationMethod } from '../shared/automation-schemas';
import { CommandArguments, CliUsageError, type GlobalOptions } from './arguments';
import { printValue } from './output';
import { resolveStartDirectory } from './start';

export type AutomationCommand =
  | { kind: 'run'; executable: string; args: string[]; cwd?: string; title?: string; requestId?: string; detached: boolean }
  | { kind: 'runs'; operation: 'list' | 'show' | 'stop'; id?: string };
export function parseAutomationCommand(args: string[]): AutomationCommand | null {
  if (args[0] === 'run') {
    const separator = args.indexOf('--');
    if (separator < 0 || !args[separator + 1]) throw new CliUsageError('Usage: shellfox run [--cwd PATH] [--title TITLE] [--detach] -- EXECUTABLE [ARG...]');
    const flags = new CommandArguments(args.slice(1, separator), ['--cwd', '--title', '--request-id'], ['--detach']); flags.arity(0);
    const requestId = flags.text('--request-id');
    if (requestId !== undefined && !automationRunSchema.shape.id.safeParse(requestId).success) throw new CliUsageError('--request-id must be a UUID.');
    return { kind: 'run', executable: args[separator + 1], args: args.slice(separator + 2), cwd: flags.text('--cwd'), title: flags.text('--title'), ...(requestId ? { requestId } : {}), detached: flags.has('--detach') };
  }
  if (args[0] !== 'runs') return null;
  const operation = args[1];
  if (!['list', 'show', 'stop'].includes(operation)) throw new CliUsageError('Usage: shellfox runs list|show RUN_UUID|stop RUN_UUID');
  const flags = new CommandArguments(args.slice(2)); flags.arity(operation === 'list' ? 0 : 1);
  if (operation === 'list') return { kind: 'runs', operation };
  const id = flags.positionals[0];
  if (!/^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i.test(id)) throw new CliUsageError('Expected a full run UUID.');
  return { kind: 'runs', operation: operation === 'show' ? 'show' : 'stop', id };
}
export interface AutomationIo { input: Readable; output: Writable; error: Writable; signals: NodeJS.EventEmitter }
async function write(stream: Writable, bytes: Buffer): Promise<void> {
  await new Promise<void>((resolve, reject) => { stream.write(bytes, error => error ? reject(error) : resolve()); });
}
function signalCode(signal: string | null): number { return signal === 'SIGINT' ? 130 : signal === 'SIGTERM' ? 143 : 137; }
export async function runAutomationCommand(command: AutomationCommand, client: Pick<HostClient, 'request'>, options: GlobalOptions,
  io: AutomationIo = { input: process.stdin, output: process.stdout, error: process.stderr, signals: process }): Promise<number> {
  if (options.host && options.host !== 'local') throw new CliUsageError('Automation commands use the local host. Run the CLI on the target machine.');
  function call<M extends AutomationMethod>(method: M, payload: unknown): Promise<HostResponse<M>>;
  async function call(method: AutomationMethod, payload: unknown): Promise<unknown> {
    return automationResponseSchemas[method].parse(await client.request(method, automationRequestSchemas[method].parse(payload)));
  }
  if (command.kind === 'runs') {
    if (command.operation === 'stop' && !options.yes) throw new CliUsageError('Stopping an automation run requires --yes.');
    const value = command.operation === 'list' ? await call('automation.list', {}) : command.operation === 'show'
      ? await call('automation.show', { id: command.id }) : await call('automation.stop', { id: command.id });
    printValue(value, options.json, io.output); return 0;
  }
  if (options.json && !command.detached) throw new CliUsageError('Use --detach with --json. Foreground runs preserve raw stdout and stderr.');
  const env: Record<string, string> = {};
  for (const [key, value] of Object.entries(process.env)) if (value !== undefined && key !== 'ELECTRON_RUN_AS_NODE') env[key] = value;
  const run = await call('automation.start', { requestId: command.requestId ?? randomUUID(), executable: command.executable, args: command.args,
    cwd: await resolveStartDirectory(command.cwd), env, title: command.title, detached: command.detached });
  if (command.detached) { printValue(run, options.json, io.output); return 0; }
  // Only command bytes go to stdout/stderr. Discover the ID from another CLI with runs list.
  let stopped = false, inputFailure: unknown;
  const failed = (error: Error) => { inputFailure = error; };
  io.output.on('error', failed); io.error.on('error', failed);
  const terminate = () => { void call('automation.stop', { id: run.id }).catch(error => { inputFailure = error; }); };
  for (const signal of ['SIGINT', 'SIGTERM', 'SIGHUP']) io.signals.on(signal, terminate);
  const iterator = io.input.iterator({ destroyOnReturn: false });
  const input = (async () => {
    try {
      for await (const chunk of iterator) {
        if (stopped) break;
        const bytes = Buffer.isBuffer(chunk) ? chunk : Buffer.from(String(chunk));
        for (let offset = 0; offset < bytes.length && !stopped; offset += 32768) await call('automation.write', { id: run.id, data: bytes.subarray(offset, offset + 32768).toString('base64'), eof: false });
      }
      if (!stopped) await call('automation.write', { id: run.id, data: '', eof: true });
    } catch (error) {
      // Like an ordinary pipe, a child can close stdin before its producer finishes.
      if (!(error instanceof Error && /stdin is closed|EPIPE|write after end/i.test(error.message))) inputFailure = error;
    }
  })();
  let after = 0;
  try {
    while (true) {
      if (inputFailure) throw inputFailure;
      const result = await call('automation.read', { id: run.id, after });
      for (const frame of result.frames) {
        await write(frame.stream === 'stdout' ? io.output : io.error, Buffer.from(frame.data, 'base64'));
        after = frame.sequence;
      }
      if (result.run.state === 'exited' && !result.frames.length) return result.run.exitCode ?? signalCode(result.run.signal);
      if (!result.frames.length) await delay(40);
    }
  } finally {
    stopped = true; io.input.pause();
    io.output.off('error', failed); io.error.off('error', failed);
    for (const signal of ['SIGINT', 'SIGTERM', 'SIGHUP']) io.signals.off(signal, terminate);
    // Do not await a pending read on an interactive stdin after the child exits.
    void iterator.return?.(); void input;
  }
}
