import { createInterface } from 'node:readline';
import type { Readable, Writable } from 'node:stream';
import { requestControl } from './control-client';
import type { ControlAction, ControlRequest, ControlResponse, TabStat } from './control-types';
import type { SessionDto } from '../shared/contracts';
import { commandHelp, escapeTerminal, formatControl, helpText } from './ssh/presentation';

const aliases: Record<string, string> = {
  add: 'start', new: 'start', create: 'start', list: 'ls', sessions: 'ls',
  remove: 'delete', rm: 'delete', keepalive: 'keep-alive', awake: 'keep-alive',
  status: 'stats', usage: 'stats', proc: 'processes', ps: 'processes',
  '--version': 'version', '-v': 'version', '--help': 'help', '-h': 'help',
};
export const canonicalCommand = (command: string): string => Object.hasOwn(aliases, command) ? aliases[command] : command;
const readActions: Record<string, ControlAction> = { ls: 'list', stats: 'stats', processes: 'processes' };
const uuid = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;
export interface ControlFilters {
  running?: boolean; idle?: boolean; noAgents?: boolean; current?: boolean;
  id?: string; name?: string; path?: string; tab?: string; pid?: number;
}
export type ParsedControlCli =
  | { kind: 'help'; topic?: string }
  | { kind: 'version' }
  | { kind: 'control'; action: ControlAction; filters: ControlFilters };

function topicFor(args: string[]): string {
  const command = canonicalCommand(args[0] ?? '');
  if (command === 'tab') {
    if (args.length === 1) return 'tab';
    const sub = ({ add: 'open', new: 'open', remove: 'close', rm: 'close' } as Record<string, string>)[args[1]] ?? args[1];
    if (args.length === 2 && ['open', 'close'].includes(sub)) return 'tab ' + sub;
  } else if (args.length === 1 && ['start', 'ls', 'delete', 'settle', 'unsettle', 'keep-alive', 'stats', 'processes', 'ssh', 'update', 'version'].includes(command)) return command;
  throw new Error('Unknown help topic');
}
export function parseControlArgs(argv: string[]): ParsedControlCli {
  const args = [...argv];
  const command = canonicalCommand(args.shift() ?? 'help');
  if (command === 'help') return { kind: 'help', ...(args.length ? { topic: topicFor(args) } : {}) };
  // Help must stand alone, so a misspelled destructive invocation never succeeds silently.
  if (args.length && ['--help', '-h', 'help'].includes(args.at(-1)!)) {
    return { kind: 'help', topic: topicFor([command, ...args.slice(0, -1)]) };
  }
  if (command === 'version' && !args.length) return { kind: 'version' };
  let action: ControlAction | undefined = Object.hasOwn(readActions, command) ? readActions[command] : undefined;
  if (command === 'tab') {
    const sub = args.shift();
    if (sub && ['open', 'add', 'new'].includes(sub)) action = 'tab-open';
    else if (sub && ['close', 'remove', 'rm'].includes(sub)) action = 'tab-close';
    else throw new Error('Expected tab open or tab close. Run shellfox help tab');
  } else if (command === 'delete' || command === 'settle' || command === 'unsettle' || command === 'keep-alive') action = command;
  if (!action) throw new Error(command === 'start' ? 'Use the shellfox launcher to start a GUI session' : 'Invalid command. Run shellfox help');
  const reading = Object.hasOwn(readActions, command);
  const tabAction = action === 'tab-open' || action === 'tab-close';
  const filters: ControlFilters = {};
  const seen = new Set<string>();
  for (let i = 0; i < args.length; i++) {
    let flag = args[i];
    let inline: string | undefined;
    const equal = flag.indexOf('=');
    if (equal >= 0) { inline = flag.slice(equal + 1); flag = flag.slice(0, equal); }
    if (!flag.startsWith('-')) {
      if (reading || tabAction || action === 'keep-alive' || filters.id || !uuid.test(flag)) throw new Error('Expected a full UUID, not a name or index');
      filters.id = flag.toLowerCase(); continue;
    }
    if (seen.has(flag)) throw new Error('Duplicate option: ' + flag);
    seen.add(flag);
    const booleanFlags: Record<string, 'running' | 'idle' | 'noAgents' | 'current'> = {
      '--running': 'running', '--idle': 'idle', '--no-agents': 'noAgents', '--current': 'current',
    };
    const booleanKey = booleanFlags[flag];
    if (booleanKey) {
      if (inline !== undefined || (!reading && flag !== '--current') || action === 'keep-alive') throw new Error('Invalid option: ' + flag);
      filters[booleanKey] = true; continue;
    }
    const valueFlags: Record<string, 'id' | 'name' | 'path' | 'tab' | 'pid'> = {
      '--id': 'id', '--name': 'name', '--path': 'path', '--tab': 'tab', '--pid': 'pid',
    };
    const key = valueFlags[flag];
    if (!key || tabAction || (!reading && key !== 'id') || action === 'keep-alive' || (key === 'pid' && action !== 'processes')) throw new Error('Invalid option: ' + flag);
    const value = inline ?? args[++i];
    if (!value || value.startsWith('--') || filters[key] !== undefined) throw new Error('Missing or duplicate value for ' + flag);
    if (key === 'id' || key === 'tab') {
      if (!uuid.test(value)) throw new Error(flag + ' requires a full UUID');
      filters[key] = value.toLowerCase();
    } else if (key === 'pid') {
      if (!/^[1-9]\d*$/.test(value) || !Number.isSafeInteger(Number(value))) throw new Error('--pid requires a positive integer');
      filters.pid = Number(value);
    } else filters[key] = value;
  }
  if (filters.running && filters.idle) throw new Error('Do not combine --running and --idle');
  if (!reading && filters.id && filters.current) throw new Error('Choose a UUID or --current, not both');
  if (!reading && action !== 'keep-alive' && !filters.id) filters.current = true;
  return { kind: 'control', action, filters };
}

export function filterControl(response: ControlResponse, filters: ControlFilters): ControlResponse {
  const match = (value: string, term?: string) => term === undefined || value.toLowerCase().includes(term.toLowerCase());
  const stats = new Map((response.tabs ?? []).map(t => [t.tabId, t]));
  const sessions = (response.sessions ?? (response.session ? [response.session] : [])).filter(s =>
    (!filters.id || s.id.toLowerCase() === filters.id) && match(s.title, filters.name) && match(s.cwd, filters.path) &&
    (!filters.running || s.activityStatus === 'running') && (!filters.idle || (s.status !== 'settled' && s.activityStatus === 'waiting')) &&
    (!filters.noAgents || s.counts.agents === 0)
  ).map(s => ({ ...s, tabs: s.tabs.filter(t => (!filters.tab || t.id.toLowerCase() === filters.tab) &&
    (filters.pid === undefined || stats.get(t.id)?.processes.some(p => p.pid === filters.pid))) }))
    .filter(s => (!filters.tab && filters.pid === undefined) || s.tabs.length > 0);
  const tabIds = new Set(sessions.flatMap(s => s.tabs.map(t => t.id)));
  const tabs = (response.tabs ?? []).filter(t => tabIds.has(t.tabId)).map(t => ({ ...t,
    processes: t.processes.filter(p => filters.pid === undefined || p.pid === filters.pid),
  }));
  return { ...response, sessions, tabs };
}

export interface ControlCliIo {
  input: Readable & { isTTY?: boolean }; output: Writable & { isTTY?: boolean };
  error: Writable; marker?: string; request: (userData: string, request: ControlRequest) => Promise<ControlResponse>;
}
export function confirmControl(question: string, input: ControlCliIo['input'], output: ControlCliIo['output']): Promise<boolean> {
  if (!input.isTTY || !output.isTTY || input.readableEnded || input.destroyed || output.destroyed || output.writableEnded) return Promise.resolve(false);
  return new Promise(resolve => {
    const rl = createInterface({ input, output, terminal: false });
    let answered = false;
    const finish = (yes: boolean) => {
      if (answered) return;
      answered = true;
      rl.close();
      input.off('error', failed);
      resolve(yes);
    };
    const failed = () => finish(false);
    input.once('error', failed);
    rl.once('error', failed);
    rl.once('close', failed);
    rl.once('SIGINT', failed);
    rl.once('line', answer => finish(!input.readableEnded && !input.destroyed && /^(y|yes)$/i.test(answer.trim())));
    output.write(question);
  });
}
function inspectionText(session: SessionDto, tabs: TabStat[]): string {
  return formatControl('list', { sessions: [session] }) +
    formatControl('processes', { sessions: [session], tabs });
}
export async function runControlCli(argv: string[], userData: string, version: string, options: Partial<ControlCliIo> & { color?: boolean; opensApp?: boolean } = {}): Promise<number> {
  const io: ControlCliIo = { input: process.stdin, output: process.stdout, error: process.stderr,
    marker: process.env.SHELLFOX_TERMINAL_MARKER, request: requestControl, ...options };
  let parsed: ParsedControlCli;
  try { parsed = parseControlArgs(argv); }
  catch (error) { io.error.write('  ' + escapeTerminal((error as Error).message) + '\n'); return 2; }
  if (parsed.kind === 'help') {
    io.output.write(parsed.topic ? commandHelp(parsed.topic, options.color ?? false) : helpText(version, options.color ?? false, options.opensApp ?? true)); return 0;
  }
  if (parsed.kind === 'version') { io.output.write(escapeTerminal(version) + '\n'); return 0; }
  const { action, filters } = parsed;
  const reading = ['list', 'stats', 'processes'].includes(action);
  const request: ControlRequest = { action, ...(filters.current && io.marker ? { marker: io.marker } : {}),
    ...(filters.id && (!reading || !filters.current) ? { id: filters.id } : {}), ...(filters.current ? { current: true } : {}) };
  if (filters.current && (!io.marker || !uuid.test(io.marker))) { io.error.write('  No current terminal. Use a UUID or run inside a Shellfox terminal.\n'); return 2; }
  try {
    if (action === 'delete') {
      const inspected = await io.request(userData, { ...request, action: 'inspect' });
      const session = inspected.session;
      if (!session || !uuid.test(session.id) || (filters.id && session.id.toLowerCase() !== filters.id)) throw new Error('Could not inspect the target session');
      io.output.write(inspectionText(session, inspected.tabs ?? []));
      io.output.write('  Delete session and close its owned terminals.\n');
      if (!await confirmControl('  ' + action + ' ' + escapeTerminal(session.id) + '? [y/N] ', io.input, io.output)) {
        io.output.write('  Cancelled. No changes made.\n'); return 1;
      }
      // Pin the inspected UUID instead of resolving the current marker a second time.
      const result = await io.request(userData, { action, id: session.id, confirmed: true });
      io.output.write('  ' + escapeTerminal(result.message ?? 'Session deleted.') + '\n');
    } else {
      const response = await io.request(userData, request);
      if (['list', 'stats', 'processes'].includes(action)) io.output.write(formatControl(action, filterControl(response, filters)));
      else io.output.write('  ' + escapeTerminal(response.message ?? 'Done.') + '\n');
    }
    return 0;
  } catch (error) { io.error.write('  ' + escapeTerminal((error as Error).message) + '\n'); return 1; }
}
