import { cliHelp } from '../../cli/help';
import { formatTable } from '../../cli/table';
import { escapeTerminal, styles, terminalWidth, wrapText } from '../../cli/terminal-text';
export { escapeTerminal, styles } from '../../cli/terminal-text';
import type { StoredProfile } from './storage';
import type { ControlResponse, ProcessStat, TabStat } from '../control-types';

export const targetLabel = (p: Pick<StoredProfile,'host'|'user'|'port'>) => escapeTerminal((p.user ? p.user + '@' : '') + (p.host.includes(':') ? '[' + p.host + ']' : p.host) + (p.port === 22 ? '' : ':' + p.port));
export function pickerLines(profiles: StoredProfile[], selected: number, color: boolean, width = 120): string[] {
  profiles = profiles.map(p => ({ ...p, name: escapeTerminal(p.name), remoteCwd: p.remoteCwd === null ? null : escapeTerminal(p.remoteCwd) }));
  const s = styles(color), names = Math.min(Math.max(...profiles.map(p => p.name.length)),Math.max(4,Math.floor(width/3))), targets = Math.min(Math.max(...profiles.map(p => targetLabel(p).length)),Math.max(5,width-names-10));
  const fit = (value:string,size:number) => value.length>size ? value.slice(0,Math.max(0,size-1))+'…' : value;
  return ['', '  ssh', ...profiles.map((p,i) => {
    const name = fit(p.name,names).padEnd(names), target = fit(targetLabel(p),targets).padEnd(targets), cwd = p.remoteCwd ?? '';
    const available = Math.max(0, width - names - targets - 10), folder = cwd.length > available ? cwd.slice(0,Math.max(0,available-1)) + '…' : cwd;
    return i === selected ? `  ${s.accent('› ' + name + '  ' + target)}${folder ? '  ' + s.dim(folder) : ''}` : `    ${name}  ${s.dim(target)}${folder ? '  ' + s.dim(folder) : ''}`;
  }), '', '  ' + s.dim('↑↓ select  enter connect  esc cancel')].flatMap(line => wrapText(line, width).split('\n'));
}
export function helpText(version: string, color: boolean, _opensApp = true, width = terminalWidth()): string { return cliHelp(version, undefined, color, width); }
export function sshHelp(color: boolean, width = terminalWidth()): string {
  const s = styles(color);
  return wrapText('\n  ' + s.accent('shellfox ssh') + ' [--profile name]\n  ' + s.dim('connect to a saved ssh connection') + '\n  ' + s.dim('without a name, choose a connection with ↑↓ and enter') + '\n', width);
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

export function commandHelp(topic: string, color: boolean, width = terminalWidth()): string {
  return topic === 'ssh' ? sshHelp(color, width) : cliHelp('', topic, color, width);
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
export function formatControl(action: string, response: ControlResponse, color = false, width = terminalWidth()): string {
  const sessions = response.sessions ?? (response.session ? [response.session] : []);
  if (!sessions.length) return '  No matching sessions.\n';
  if (action === 'list') {
    const rows = sessions.map(s => [escapeTerminal(s.title), escapeTerminal(s.id), s.status, String(s.counts.agents), escapeTerminal(s.cwd), String(s.tabs.length)]);
    return formatTable(['NAME', 'ID', 'STATUS', 'AGENTS', 'PATH', 'TERMINALS'], rows, color, Math.max(1, width - 2))
      .split('\n').map(line => '  ' + line).join('\n') + '\n';
  }
  const stats = new Map((response.tabs ?? []).map(t => [t.tabId, t]));
  const lines: string[] = [];
  for (const session of sessions) {
    // A closed saved tab may have no runtime stats. Retain stats when owned
    // descendant cleanup is pending, and retain missing live stats as unknown.
    const tabs = session.tabs.filter(t => t.lifecycle !== 'closed' || stats.has(t.id)).map(t => stats.get(t.id));
    const metrics = '  RSS ' + formatRss(aggregate(tabs, 'memoryBytes')) + '  duration ' + formatDuration(aggregate(tabs, 'elapsedSeconds'));
    lines.push('  ' + styles(color).accent(escapeTerminal(session.title)) + '  ' + escapeTerminal(session.id) + metrics);
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
  return wrapText(lines.join('\n'), width) + '\n';
}
