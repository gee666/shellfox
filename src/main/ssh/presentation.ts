import type { StoredProfile } from './storage';
import type { ControlResponse, ProcessStat, TabStat } from '../control-types';

/** Render control bytes visibly; never let names, paths or errors issue terminal commands. */
export const escapeTerminal = (value: string): string => value.replace(/[\x00-\x1f\x7f-\x9f\u2028\u2029\u202a-\u202e\u2066-\u2069]/g,
  c => '\\u' + c.charCodeAt(0).toString(16).padStart(4, '0'));
export const styles = (color: boolean) => {
  const wrap = (code: string) => (s: string) => color ? `\x1b[${code}m${s}\x1b[0m` : s;
  return { accent: wrap('38;2;236;72;153'), dim: wrap('2'), bold: wrap('1'), red: wrap('31') };
};
export const targetLabel = (p: Pick<StoredProfile,'host'|'user'|'port'>) => escapeTerminal((p.user ? p.user + '@' : '') + (p.host.includes(':') ? '[' + p.host + ']' : p.host) + (p.port === 22 ? '' : ':' + p.port));
export function pickerLines(profiles: StoredProfile[], selected: number, color: boolean, width = 120): string[] {
  profiles = profiles.map(p => ({ ...p, name: escapeTerminal(p.name), remoteCwd: p.remoteCwd === null ? null : escapeTerminal(p.remoteCwd) }));
  const s = styles(color), names = Math.min(Math.max(...profiles.map(p => p.name.length)),Math.max(4,Math.floor(width/3))), targets = Math.min(Math.max(...profiles.map(p => targetLabel(p).length)),Math.max(5,width-names-10));
  const fit = (value:string,size:number) => value.length>size ? value.slice(0,Math.max(0,size-1))+'…' : value;
  return ['', '  ssh', ...profiles.map((p,i) => {
    const name = fit(p.name,names).padEnd(names), target = fit(targetLabel(p),targets).padEnd(targets), cwd = p.remoteCwd ?? '';
    const available = Math.max(0, width - names - targets - 10), folder = cwd.length > available ? cwd.slice(0,Math.max(0,available-1)) + '…' : cwd;
    return i === selected ? `  ${s.accent('› ' + name + '  ' + target)}${folder ? '  ' + s.dim(folder) : ''}` : `    ${name}  ${s.dim(target)}${folder ? '  ' + s.dim(folder) : ''}`;
  }), '', '  ' + s.dim('↑↓ select  enter connect  esc cancel')];
}
export function helpText(version: string, color: boolean, opensApp = true): string {
  const s = styles(color);
  const rows = [
    ...(opensApp ? [['shellfox', 'open shellfox']] : []),
    ['shellfox start [path]', 'new session; aliases: add, new, create'],
    ['shellfox ls [filters]', 'sessions; aliases: list, sessions'],
    ['shellfox stats [filters]', 'RSS and duration; aliases: status, usage'],
    ['shellfox processes [filters]', 'process PIDs; aliases: proc, ps'],
    ['shellfox delete [UUID]', 'inspect and confirm; aliases: remove, rm'],
    ['shellfox archive [UUID]', 'inspect and confirm; alias: settle'],
    ['shellfox tab open', 'open tab here; aliases: add, new'],
    ['shellfox tab close', 'close this tab; aliases: remove, rm'],
    ['shellfox keep-alive', 'toggle sleep prevention; aliases: keepalive, awake'],
    ['shellfox ssh [--profile name]', 'connect to a saved ssh connection'],
    ['shellfox update [--check]', 'install the latest release'],
    ['shellfox --version', 'version; aliases: -v, version'],
    ['shellfox help [command]', 'help; aliases: --help, -h'],
  ];
  return ['','  ' + s.bold(s.accent('shellfox')) + ' ' + s.dim(escapeTerminal(version)) + '  ' + s.dim('terminal manager'), '',
    ...rows.map(([cmd, description]) => '    ' + s.accent(cmd.padEnd(32)) + s.dim(description)), '',
    '  Filters: --running --idle --no-agents --current --id UUID --name TEXT --path TEXT --tab UUID',
    '  processes also accepts --pid PID. Filters combine with AND; --running and --idle conflict.',
    '  Run shellfox help <command> for semantics. IDs are full UUIDs, never row numbers.', '', ''].join('\n');
}
export function sshHelp(color: boolean): string {
  const s = styles(color);
  return '\n  ' + s.accent('shellfox ssh') + ' [--profile name]\n  ' + s.dim('connect to a saved ssh connection') + '\n  ' + s.dim('without a name, choose a connection with ↑↓ and enter') + '\n';
}
export function parseSshArgs(args: string[]): { profile?: string; help?: boolean } {
  if(args.length===1&&['--help','-h','help'].includes(args[0]))return {help:true};
  if(!args.length)return {};
  if(args.length===1&&args[0]&&!args[0].startsWith('-'))return {profile:args[0]};
  if(args.length===2&&['--profile','-p'].includes(args[0])&&args[1]&&!args[1].startsWith('-'))return {profile:args[1]};
  throw new Error('invalid ssh arguments');
}
export const quoteShell = (s: string) => "'" + s.replaceAll("'", "'\\''") + "'";
export function remoteCommand(cwd: string | null, color = false): string {
  const directory = cwd === '~' ? '"$HOME"' : cwd?.startsWith('~/') ? '"$HOME"/' + quoteShell(cwd.slice(2)) : quoteShell(cwd ?? '~');
  const warning = color ? '\\033[2m  remote folder unavailable; staying in home\\033[0m' : '  remote folder unavailable; staying in home';
  const prefix = cwd ? `cd -- ${directory} 2>/dev/null || printf '${warning}\\n' >&2; ` : '';
  return prefix+'exec "${SHELL:-/bin/sh}" -il';
}

const filterHelp = [
  '  --running       session activity is running',
  '  --idle          live session activity is waiting, not unknown/error/archived',
  '  --no-agents     reported session agent count is zero, not proof of no processes',
  '  --current       session containing SHELLFOX_TERMINAL_MARKER; fails outside Shellfox',
  '  --id UUID       exact session UUID',
  '  --name TEXT     case-insensitive substring of session name',
  '  --path TEXT     case-insensitive substring of starting path, not live shell CWD',
  '  --tab UUID      exact tab UUID; show only that tab and its containing session',
  '  Filters combine with AND. --running and --idle cannot be combined.',
  '  Use full UUIDs from ls/stats, never row numbers or UUID prefixes.',
];
export function commandHelp(topic: string, color: boolean): string {
  if (topic === 'ssh') return sshHelp(color);
  const s = styles(color);
  const aliases: Record<string, string> = {
    start: 'add, new, create', ls: 'list, sessions', delete: 'remove, rm', archive: 'settle',
    stats: 'status, usage', processes: 'proc, ps', 'keep-alive': 'keepalive, awake',
    'tab open': 'tab add, tab new', 'tab close': 'tab remove, tab rm', version: '--version, -v',
  };
  const lines: Record<string, string[]> = {
    start: ['shellfox start [path]', '  Create a GUI session in path. Default: caller current folder.'],
    ls: ['shellfox ls [filters]', '  List session name, UUID, starting path and tab count.', ...filterHelp],
    stats: ['shellfox stats [filters]', '  Session and tab RSS and duration. Unavailable metrics are n/a.',
      '  Session RSS sums included tabs; duration is their oldest measured shell age.',
      '  Closed tabs without stats are excluded; closed tabs with cleanup stats still count.',
      '  Missing live-tab metrics keep the corresponding session metric n/a.', ...filterHelp],
    processes: ['shellfox processes [filters]', '  Session > tab > process hierarchy with PID, PPID, domain, RSS and duration.',
      ...filterHelp, '  --pid PID        exact positive PID within each tab; may match multiple domains',
      '  --pid filters rows and their containing tabs/sessions, not full-tab metrics.',
      '  Session totals aggregate included tabs, not just the matching process rows.',
      '  Closed tabs without stats are excluded from totals; missing live stats remain n/a.',
      '  n/a means unavailable, not zero. Processes are observed owned trees, not the whole OS.'],
    delete: ['shellfox delete [UUID | --id UUID | --current]', '  Delete the session and close owned terminals after inspection.',
      '  Without a UUID, target the current Shellfox terminal session.', '  Requires TTY input/output and explicit y or yes. EOF, other answers and pipes cancel.'],
    archive: ['shellfox archive [UUID | --id UUID | --current]', '  Archive the session and close owned terminals after inspection.',
      '  Without a UUID, target the current Shellfox terminal session.', '  Requires TTY input/output and explicit y or yes. EOF, other answers and pipes cancel.'],
    tab: ['shellfox tab open [--current]', '  shellfox tab close [--current]', '  open aliases: add, new. close aliases: remove, rm.',
      '  Requires the current Shellfox terminal marker. UUID targets are not accepted.'],
    'tab open': ['shellfox tab open [--current]', '  Open a tab in the current session with its defaults. Requires a Shellfox terminal marker.'],
    'tab close': ['shellfox tab close [--current]', '  Close the invoking owned Shellfox tab. Requires its live terminal marker.'],
    'keep-alive': ['shellfox keep-alive', '  Toggle the existing app sleep-prevention switch. Not persisted across app restarts.'],
    version: ['shellfox --version', '  Print the installed version without contacting the app.'],
    update: ['shellfox update [--check]', '  Install the latest release; --check only checks.'],
  };
  const [usage, ...body] = lines[topic] ?? [];
  return '\n  ' + s.accent(usage ?? 'shellfox help') + '\n' + body.join('\n') + '\n' +
    (aliases[topic] ? '  Aliases: ' + aliases[topic] + '\n' : '') + '  Help: --help, -h, help, or shellfox help ' + topic + '\n';
}

export const formatRss = (bytes: number | null | undefined): string =>
  bytes == null || !Number.isFinite(bytes) || bytes < 0 ? 'n/a' : (bytes / 1048576).toFixed(1) + ' MiB';
export function formatDuration(seconds: number | null | undefined): string {
  if (seconds == null || !Number.isFinite(seconds) || seconds < 0) return 'n/a';
  const total = Math.floor(seconds), hours = Math.floor(total / 3600), minutes = Math.floor(total / 60) % 60;
  return (hours ? hours + 'h ' : '') + (hours || minutes ? minutes + 'm ' : '') + total % 60 + 's';
}
function aggregate(tabs: (TabStat | undefined)[], key: 'memoryBytes' | 'elapsedSeconds'): number | null {
  if (!tabs.length || tabs.some(t => t?.[key] == null)) return null;
  const values = tabs.map(t => t![key]!);
  return key === 'memoryBytes' ? values.reduce((sum, value) => sum + value, 0) : Math.max(...values);
}
function processLines(processes: ProcessStat[]): string[] {
  const result: string[] = [], seen = new Set<ProcessStat>();
  const sorted = [...processes].sort((a, b) => a.domain.localeCompare(b.domain) || a.pid - b.pid);
  const key = (domain: string, pid: number) => JSON.stringify([domain, pid]);
  const identities = new Set(sorted.map(p => key(p.domain, p.pid)));
  const children = new Map<string, ProcessStat[]>();
  for (const p of sorted) {
    const parent = key(p.domain, p.parentPid);
    const rows = children.get(parent) ?? [];
    rows.push(p); children.set(parent, rows);
  }
  const visit = (root: ProcessStat) => {
    const stack = [{ p: root, depth: 0 }];
    while (stack.length) {
      const { p, depth } = stack.pop()!;
      if (seen.has(p)) continue;
      seen.add(p);
      result.push('      ' + '  '.repeat(Math.min(depth, 20)) + '+- ' + escapeTerminal(p.name) +
        '  PID ' + p.pid + '  PPID ' + p.parentPid + '  ' + escapeTerminal(p.domain) +
        '  RSS ' + formatRss(p.memoryBytes) + '  duration ' + formatDuration(p.elapsedSeconds));
      const rows = children.get(key(p.domain, p.pid)) ?? [];
      for (let i = rows.length - 1; i >= 0; i--) stack.push({ p: rows[i], depth: depth + 1 });
    }
  };
  for (const p of sorted) if (!identities.has(key(p.domain, p.parentPid))) visit(p);
  for (const p of sorted) visit(p); // Cycles and missing parents must not hide processes.
  return result;
}
export function formatControl(action: string, response: ControlResponse): string {
  const sessions = response.sessions ?? (response.session ? [response.session] : []);
  if (!sessions.length) return '  No matching sessions.\n';
  if (action === 'list') {
    const rows = [['NAME', 'ID', 'PATH', 'TABS'], ...sessions.map(s =>
      [escapeTerminal(s.title), escapeTerminal(s.id), escapeTerminal(s.cwd), String(s.tabs.length)])];
    const widths = rows[0].map((_, i) => Math.max(...rows.map(row => row[i].length)));
    return rows.map(row => '  ' + row.map((cell, i) => i === row.length - 1 ? cell : cell.padEnd(widths[i])).join('  ')).join('\n') + '\n';
  }
  const stats = new Map((response.tabs ?? []).map(t => [t.tabId, t]));
  const lines: string[] = [];
  for (const session of sessions) {
    // A closed saved tab may have no runtime stats. Retain stats when owned
    // descendant cleanup is pending, and retain missing live stats as unknown.
    const tabs = session.tabs.filter(t => t.lifecycle !== 'closed' || stats.has(t.id)).map(t => stats.get(t.id));
    const metrics = '  RSS ' + formatRss(aggregate(tabs, 'memoryBytes')) + '  duration ' + formatDuration(aggregate(tabs, 'elapsedSeconds'));
    lines.push('  ' + escapeTerminal(session.title) + '  ' + escapeTerminal(session.id) + metrics);
    if (!session.tabs.length) lines.push('    No tabs.');
    for (const tab of session.tabs) {
      const stat = stats.get(tab.id);
      lines.push('    +- ' + escapeTerminal(tab.title) + '  ' + escapeTerminal(tab.id) +
        '  RSS ' + formatRss(stat?.memoryBytes) + '  duration ' + formatDuration(stat?.elapsedSeconds));
      if (stat?.reason || tab.monitoringReason) lines.push('       ' + escapeTerminal(stat?.reason ?? tab.monitoringReason!));
      if (action === 'processes') {
        const processes = stat?.processes ?? [];
        lines.push(...processLines(processes));
        if (!processes.length) lines.push(stat ? '       No observed processes.' : '       Processes n/a.');
      }
    }
  }
  return lines.join('\n') + '\n';
}
