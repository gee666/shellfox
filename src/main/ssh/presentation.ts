import type { StoredProfile } from './storage';
export const styles = (color: boolean) => {
  const wrap = (code: string) => (s: string) => color ? `\x1b[${code}m${s}\x1b[0m` : s;
  return { accent: wrap('38;2;236;72;153'), dim: wrap('2'), bold: wrap('1'), red: wrap('31') };
};
export const targetLabel = (p: Pick<StoredProfile,'host'|'user'|'port'>) => (p.user ? p.user + '@' : '') + (p.host.includes(':') ? '[' + p.host + ']' : p.host) + (p.port === 22 ? '' : ':' + p.port);
export function pickerLines(profiles: StoredProfile[], selected: number, color: boolean, width = 120): string[] {
  const s = styles(color), names = Math.min(Math.max(...profiles.map(p => p.name.length)),Math.max(4,Math.floor(width/3))), targets = Math.min(Math.max(...profiles.map(p => targetLabel(p).length)),Math.max(5,width-names-10));
  const fit = (value:string,size:number) => value.length>size ? value.slice(0,Math.max(0,size-1))+'…' : value;
  return ['', '  ssh', ...profiles.map((p,i) => {
    const name = fit(p.name,names).padEnd(names), target = fit(targetLabel(p),targets).padEnd(targets), cwd = p.remoteCwd ?? '';
    const available = Math.max(0, width - names - targets - 10), folder = cwd.length > available ? cwd.slice(0,Math.max(0,available-1)) + '…' : cwd;
    return i === selected ? `  ${s.accent('› ' + name + '  ' + target)}${folder ? '  ' + s.dim(folder) : ''}` : `    ${name}  ${s.dim(target)}${folder ? '  ' + s.dim(folder) : ''}`;
  }), '', '  ' + s.dim('↑↓ select  enter connect  esc cancel')];
}
export function helpText(version: string, color: boolean, opensApp = true): string {
  const s = styles(color), row = (cmd: string,args: string,description: string) => '    ' + s.accent(cmd) + args + s.dim(description);
  return [ '', '  ' + s.bold(s.accent('shellfox')) + ' ' + s.dim(version) + '  ' + s.dim('terminal manager'), '', '  ' + s.dim('usage'),
    ...(opensApp ? [row('shellfox','                       ','open shellfox')] : []),
    row('shellfox start',' [path]          ','new session in path (default: current folder)'),
    row('shellfox ssh',' [--profile name]  ','connect to a saved ssh connection'),
    row('shellfox update',' [--check]      ','install the latest release'), '', '  ' + s.dim('examples'),
    '    ' + s.accent('shellfox start') + ' .', '    ' + s.accent('shellfox ssh') + ' --profile prod-web', '', '' ].join('\n');
}
export function sshHelp(color: boolean): string {
  const s = styles(color);
  return '\n  ' + s.accent('shellfox ssh') + ' [--profile name]\n  ' + s.dim('connect to a saved ssh connection') + '\n  ' + s.dim('without a name, choose a connection with ↑↓ and enter') + '\n';
}
export function parseSshArgs(args: string[]): { profile?: string; help?: boolean } {
  if(args.length===1&&args[0]==='--help')return {help:true};
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
