import { createInterface } from 'node:readline';
import type { Readable, Writable } from 'node:stream';
import { requestControl } from './control-client';
import type { ControlAction, ControlRequest, ControlResponse, TabStat } from './control-types';
import type { SessionDto } from '../shared/contracts';
import { commandHelp, escapeTerminal, formatControl, helpText } from './ssh/presentation';
import { CliUsageError } from '../cli/arguments';
import { printError, printValue } from '../cli/output';
import { colorEnabled } from '../cli/human';
import { knownHelpTopic } from '../cli/help';
import { terminalWidth } from '../cli/terminal-text';

const aliases: Record<string, string> = { ss: 'session', sessions: 'session', '--version': 'version', '-v': 'version', '--help': 'help', '-h': 'help' };
export const canonicalCommand = (command: string): string => Object.hasOwn(aliases, command) ? aliases[command] : command;
const uuid = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;
export interface ControlFilters {
  running?: boolean; idle?: boolean; noAgents?: boolean; current?: boolean; all?: boolean;
  id?: string; name?: string; path?: string; tab?: string; pid?: number;
}
export type ParsedControlCli =
  | { kind: 'help'; topic?: string }
  | { kind: 'version' }
  | { kind: 'control'; action: ControlAction; filters: ControlFilters };
export function parseControlArgs(argv: string[]): ParsedControlCli {
  const args = [...argv], namespace = canonicalCommand(args.shift() ?? 'help');
  if (namespace === 'help') {
    if (args.length && !knownHelpTopic(args[0])) throw new CliUsageError('Unknown help group.');
    return { kind: 'help', ...(args.length ? { topic: args.join(' ') } : {}) };
  }
  if (namespace === 'version' && !args.length) return { kind: 'version' };
  if (namespace !== 'session') throw new CliUsageError('Unknown command. Use shellfox session <operation> or shellfox terminals <operation>.');
  let operation = args.shift();
  if (!operation || ['help', '--help', '-h'].includes(operation)) {
    if (args.length) throw new CliUsageError('Unexpected arguments after session help.');
    return { kind: 'help', topic: 'session' };
  }
  if (operation === 'rm' || operation === '-rm') operation = 'delete';
  if (!['list', 'stats', 'processes', 'delete', 'settle', 'unsettle'].includes(operation)) throw new CliUsageError('Unknown session operation.');
  if (args.length === 1 && ['help', '--help', '-h'].includes(args[0])) return { kind: 'help', topic: 'session ' + operation };
  const action = operation === 'list' ? 'list' : operation === 'stats' ? 'stats' : operation === 'processes' ? 'processes' : operation === 'delete' ? 'delete' : operation === 'settle' ? 'settle' : 'unsettle';
  const reading = ['list', 'stats', 'processes'].includes(action), filters: ControlFilters = {}, seen = new Set<string>();
  for (let i = 0; i < args.length; i++) {
    let flag = args[i], inline: string | undefined;
    const equal = flag.indexOf('=');
    if (equal >= 0) { inline = flag.slice(equal + 1); flag = flag.slice(0, equal); }
    if (!flag.startsWith('-')) {
      if (filters.id || !uuid.test(flag)) throw new CliUsageError('Expected one full session UUID.');
      filters.id = flag.toLowerCase(); continue;
    }
    if (seen.has(flag)) throw new CliUsageError('Duplicate option: ' + flag);
    seen.add(flag);
    const booleans: Record<string, 'running' | 'idle' | 'noAgents' | 'current' | 'all'> = { '--running': 'running', '--idle': 'idle', '--no-agents': 'noAgents', '--current': 'current', '--all': 'all' };
    const booleanKey = booleans[flag];
    if (booleanKey) {
      if (inline !== undefined || (!reading && flag !== '--current')) throw new CliUsageError('Invalid option: ' + flag);
      filters[booleanKey] = true; continue;
    }
    const values: Record<string, 'id' | 'name' | 'path' | 'tab' | 'pid'> = { '--id': 'id', '--name': 'name', '--path': 'path', '--tab': 'tab', '--pid': 'pid' };
    const key = values[flag], value = inline ?? args[++i];
    if (!key || (!reading && key !== 'id') || (key === 'pid' && action !== 'processes')) throw new CliUsageError('Invalid option: ' + flag);
    if (!value || value === '--' || filters[key] !== undefined) throw new CliUsageError('Missing or duplicate value for ' + flag);
    if (key === 'id' || key === 'tab') {
      if (!uuid.test(value)) throw new CliUsageError(flag + ' requires a full UUID');
      filters[key] = value.toLowerCase();
    } else if (key === 'pid') {
      if (!/^[1-9]\d*$/.test(value) || !Number.isSafeInteger(Number(value))) throw new CliUsageError('--pid requires a positive integer');
      filters.pid = Number(value);
    } else filters[key] = value;
  }
  if (filters.running && filters.idle) throw new CliUsageError('Do not combine --running and --idle');
  if (filters.all && (filters.id || filters.current)) throw new CliUsageError('Choose --all or one session target.');
  if (filters.id) delete filters.current;
  else if (!filters.all && action !== 'list') filters.current = true;
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
  input: Readable & { isTTY?: boolean }; output: Writable & { isTTY?: boolean; columns?: number };
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
function inspectionText(session: SessionDto, tabs: TabStat[], width: number): string {
  return formatControl('list', { sessions: [session] }, false, width) +
    formatControl('processes', { sessions: [session], tabs }, false, width);
}
export async function runControlCli(argv: string[], userData: string, version: string, options: Partial<ControlCliIo> & { color?: boolean; opensApp?: boolean; json?: boolean; yes?: boolean } = {}): Promise<number> {
  const io: ControlCliIo = { input: process.stdin, output: process.stdout, error: process.stderr,
    marker: process.env.SHELLFOX_TERMINAL_MARKER, request: requestControl, ...options };
  let parsed: ParsedControlCli;
  try { parsed = parseControlArgs(argv); }
  catch (error) {
    return printError(new CliUsageError(error instanceof Error ? error.message : 'Invalid command.'), options.json ?? false, io.output, io.error);
  }
  if (parsed.kind === 'help') {
    const text = parsed.topic ? commandHelp(parsed.topic, options.color ?? false, terminalWidth(io.output)) : helpText(version, options.color ?? false, options.opensApp ?? true, terminalWidth(io.output));
    if (options.json) printValue(text, true, io.output); else io.output.write(text);
    return 0;
  }
  if (parsed.kind === 'version') { if (options.json) printValue(version, true, io.output); else io.output.write(escapeTerminal(version) + '\n'); return 0; }
  const { action, filters } = parsed;
  const reading = ['list', 'stats', 'processes'].includes(action);
  const request: ControlRequest = { action, ...(filters.current && io.marker ? { marker: io.marker } : {}),
    ...(filters.id && (!reading || !filters.current) ? { id: filters.id } : {}), ...(filters.current ? { current: true } : {}) };
  if (filters.current && (!io.marker || !uuid.test(io.marker))) {
    const message = 'No current terminal. Use a UUID or run inside a Shellfox terminal.';
    return printError(new CliUsageError(message), options.json ?? false, io.output, io.error);
  }
  try {
    if (action === 'delete') {
      if (!options.yes && (options.json || !io.input.isTTY || !io.output.isTTY)) throw new CliUsageError('session delete requires --yes in noninteractive or JSON mode.');
      const inspected = await io.request(userData, { ...request, action: 'inspect' });
      const session = inspected.session;
      if (!session || !uuid.test(session.id) || (filters.id && session.id.toLowerCase() !== filters.id)) throw new Error('Could not inspect the target session');
      if (!options.json) {
        io.output.write(inspectionText(session, inspected.tabs ?? [], terminalWidth(io.output)));
        io.output.write('  Delete session and close its owned terminals.\n');
      }
      if (!options.yes && !await confirmControl('  ' + action + ' ' + escapeTerminal(session.id) + '? [y/N] ', io.input, io.output)) {
        io.output.write('  Cancelled. No changes made.\n'); return 1;
      }
      // Pin the inspected UUID instead of resolving the current marker a second time.
      const result = await io.request(userData, { action, id: session.id, confirmed: true });
      if (options.json) printValue({ deleted: true }, true, io.output); else io.output.write('  ' + escapeTerminal(result.message ?? 'Session deleted.') + '\n');
    } else {
      const response = await io.request(userData, request);
      const result = ['list', 'stats', 'processes'].includes(action) ? filterControl(response, filters) : response;
      if (options.json) printValue(action === 'list' ? result.sessions ?? [] : action === 'settle' || action === 'unsettle' ? result.session : result, true, io.output);
      else if (['list', 'stats', 'processes'].includes(action)) io.output.write(formatControl(action, result, options.color ?? colorEnabled(io.output), terminalWidth(io.output)));
      else io.output.write('  ' + escapeTerminal(response.message ?? 'Done.') + '\n');
    }
    return 0;
  } catch (error) {
    return printError(error, options.json ?? false, io.output, io.error);
  }
}
