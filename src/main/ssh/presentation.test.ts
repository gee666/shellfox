import { expect, it } from 'vitest';
import { parseSshArgs, pickerLines, helpText, sshHelp, remoteCommand, commandHelp, escapeTerminal, formatControl, formatDuration, formatRss } from './presentation';
import type { SessionDto } from '../../shared/contracts';
import { filterControl } from '../control-cli';
import type { TabStat } from '../control-types';
import type { StoredProfile } from './storage';
const p=(name:string,host:string,user:string,port=22,remoteCwd:string|null=null):StoredProfile=>({id:name,name,host,user,port,remoteCwd,password:null,keyFile:null,source:'manual'});
it('accepts the three profile forms and rejects removed/ambiguous options',()=>{
 expect(parseSshArgs([])).toEqual({});expect(parseSshArgs(['--help'])).toEqual({help:true});
 for(const args of [['prod'],['--profile','prod'],['-p','prod']])expect(parseSshArgs(args)).toEqual({profile:'prod'});
 for(const args of [['--profile'],['--unknown'],['a','b'],['--help','a'],['-p',''],['--persist'],['prod','--persist']])expect(()=>parseSshArgs(args)).toThrow();
});
it('matches the compact picker design and removes color without a TTY',()=>{
 const profiles=[p('prod-web','10.0.0.5','deploy',22,'/var/www'),p('staging','stage.example.com','root',2222),p('nas','192.168.1.20','admin')];
 const plain=pickerLines(profiles,0,false).join('\n');expect(plain).toBe('\n  ssh\n  › prod-web  deploy@10.0.0.5              /var/www\n    staging   root@stage.example.com:2222\n    nas       admin@192.168.1.20         \n\n  ↑↓ select  enter connect  esc cancel');
 expect(plain).not.toContain('\x1b');expect(pickerLines(profiles,1,true).join('\n')).toContain('\x1b[38;2;236;72;153m› staging');
});
it('documents commands, aliases and filters while retaining four-line SSH help',()=>{
 const help = helpText('0.4.0', false);
 for (const command of ['start', 'ls', 'stats', 'processes', 'delete', 'settle', 'unsettle', 'tab open', 'tab close', 'keep-alive', 'ssh', 'update', '--version', 'help']) expect(help).toContain('shellfox ' + command);
 for (const alias of ['add', 'new', 'create', 'list', 'sessions', 'remove', 'rm', 'status', 'usage', 'proc', 'ps', 'keepalive', 'awake', '-v', '-h']) expect(help).toContain(alias);
 expect(help).not.toContain('\x1b'); expect(help).not.toContain('archive');
 expect(helpText('0.4.0',false,false)).not.toContain('open shellfox');expect(sshHelp(false).split('\n')).toHaveLength(5);
 for (const topic of ['ls', 'stats', 'processes']) {
  const text = commandHelp(topic, false);
  for (const flag of ['--running', '--idle', '--no-agents', '--current', '--id', '--name', '--path', '--tab']) expect(text).toContain(flag);
  expect(text).toContain('AND'); expect(text).toContain('UUID');
 }
 expect(commandHelp('processes', false)).toContain('--pid');
 expect(commandHelp('processes', false)).toContain('not full-tab metrics');
 expect(commandHelp('stats', false)).toContain('Closed tabs without stats are excluded');
 expect(commandHelp('stats', false)).toContain('closed tabs with cleanup stats still count');
 expect(commandHelp('delete', false)).toContain('EOF'); expect(commandHelp('delete', false)).toContain('TTY');
 for (const topic of ['settle', 'unsettle']) {
  const text = commandHelp(topic, false);
  expect(text).toContain('No confirmation'); expect(text).toContain('--current');
  expect(text).not.toContain('TTY'); expect(text).not.toContain('archive');
 }
 for (const flag of ['--help', '-h', 'help']) expect(parseSshArgs([flag])).toEqual({help:true});
});
it('renders unavailable metrics as n/a, not zero', () => {
 expect(formatRss(null)).toBe('n/a'); expect(formatRss(undefined)).toBe('n/a'); expect(formatRss(0)).toBe('0.0 MiB');
 expect(formatRss(1048576)).toBe('1.0 MiB'); expect(formatRss(NaN)).toBe('n/a');
 expect(formatDuration(null)).toBe('n/a'); expect(formatDuration(0)).toBe('0s'); expect(formatDuration(3661.9)).toBe('1h 1m 1s');
});
const sessionOutput = { id: 'session-uuid', title: 'test', cwd: '/work', tabs: [
 { id: 'tab-uuid', title: 'bash', lifecycle: 'open', monitoringReason: null },
 { id: 'tab-two', title: 'zsh', lifecycle: 'open', monitoringReason: null },
] } as SessionDto;
it('formats an aligned ls table with full IDs and paths', () => {
 const text = formatControl('list', { sessions: [sessionOutput] });
 expect(text).toBe('  NAME  ID            PATH   TABS\n  test  session-uuid  /work  2\n');
 expect(formatControl('list', {})).toBe('  No matching sessions.\n');
});
it('formats session/tab totals without treating missing metrics as zero', () => {
 const tabs = [
  { tabId: 'tab-uuid', processes: [], memoryBytes: 1048576, elapsedSeconds: 60 },
  { tabId: 'tab-two', processes: [], memoryBytes: 2097152, elapsedSeconds: 120 },
 ];
 const text = formatControl('stats', { sessions: [sessionOutput], tabs });
 expect(text).toContain('test  session-uuid  RSS 3.0 MiB  duration 2m 0s');
 expect(text).toContain('+- bash  tab-uuid  RSS 1.0 MiB  duration 1m 0s');
 expect(formatControl('stats', { sessions: [sessionOutput], tabs: [tabs[0]] })).toContain('session-uuid  RSS n/a  duration n/a');
 expect(formatControl('stats', { sessions: [sessionOutput], tabs: [{ ...tabs[0], reason: 'unavailable' }] })).toContain('unavailable');
});
it('excludes closed tabs without stats from session totals but keeps their displayed rows', () => {
 const session = { ...sessionOutput, tabs: [sessionOutput.tabs[0], { ...sessionOutput.tabs[1], lifecycle: 'closed' as const }] };
 const tabs = [{ tabId: 'tab-uuid', processes: [], memoryBytes: 1048576, elapsedSeconds: 60 }];
 for (const action of ['stats', 'processes']) {
  const text = formatControl(action, { sessions: [session], tabs });
  expect(text).toContain('test  session-uuid  RSS 1.0 MiB  duration 1m 0s');
  expect(text).toContain('+- zsh  tab-two  RSS n/a  duration n/a');
 }
 expect(formatControl('list', { sessions: [session] })).toContain('/work  2');
});
it('includes closed tabs with stats while descendant cleanup is pending', () => {
 const session = { ...sessionOutput, tabs: [sessionOutput.tabs[0], { ...sessionOutput.tabs[1], lifecycle: 'closed' as const }] };
 const tabs: TabStat[] = [
  { tabId: 'tab-uuid', processes: [], memoryBytes: 1048576, elapsedSeconds: 60 },
  { tabId: 'tab-two', processes: [], memoryBytes: 2097152, elapsedSeconds: 120, reason: 'Owned descendant cleanup is pending.' },
 ];
 for (const action of ['stats', 'processes']) {
  const text = formatControl(action, { sessions: [session], tabs });
  expect(text).toContain('test  session-uuid  RSS 3.0 MiB  duration 2m 0s');
  expect(text).toContain('+- zsh  tab-two  RSS 2.0 MiB  duration 2m 0s');
  expect(text).toContain('Owned descendant cleanup is pending.');
  expect(formatControl(action, { sessions: [session], tabs: [tabs[0], { ...tabs[1], memoryBytes: null }] }))
   .toContain('session-uuid  RSS n/a  duration 2m 0s');
  expect(formatControl(action, { sessions: [session], tabs: [tabs[0], { ...tabs[1], elapsedSeconds: null }] }))
   .toContain('session-uuid  RSS 3.0 MiB  duration n/a');
 }
});
it.each(['open', 'launching', 'launch-uncertain'] as const)('keeps missing %s tab stats unavailable even alongside excluded closed tabs', lifecycle => {
 const session = { ...sessionOutput, tabs: [
  { ...sessionOutput.tabs[0], lifecycle }, { ...sessionOutput.tabs[1], lifecycle: 'closed' as const },
 ] };
 for (const action of ['stats', 'processes']) expect(formatControl(action, { sessions: [session], tabs: [] }))
  .toContain('session-uuid  RSS n/a  duration n/a');
});
it('renders all-closed sessions without measurements as n/a, but accepts measured zero', () => {
 const session = { ...sessionOutput, tabs: sessionOutput.tabs.map(t => ({ ...t, lifecycle: 'closed' as const })) };
 for (const action of ['stats', 'processes']) {
  expect(formatControl(action, { sessions: [session] })).toContain('session-uuid  RSS n/a  duration n/a');
  expect(formatControl(action, { sessions: [session], tabs: [{ tabId: 'tab-uuid', processes: [], memoryBytes: 0, elapsedSeconds: 0 }] }))
   .toContain('session-uuid  RSS 0.0 MiB  duration 0s');
 }
});
it.each([
 { memoryBytes: null, elapsedSeconds: 60, metrics: 'RSS n/a  duration 1m 0s' },
 { memoryBytes: 1048576, elapsedSeconds: null, metrics: 'RSS 1.0 MiB  duration n/a' },
])('keeps each missing live metric unavailable independently: $metrics', ({ memoryBytes, elapsedSeconds, metrics }) => {
 const session = { ...sessionOutput, tabs: [sessionOutput.tabs[0], { ...sessionOutput.tabs[1], lifecycle: 'closed' as const }] };
 for (const action of ['stats', 'processes']) expect(formatControl(action, { sessions: [session], tabs: [{ tabId: 'tab-uuid', processes: [], memoryBytes, elapsedSeconds }] }))
  .toContain('session-uuid  ' + metrics);
});
it('filters PID rows without recomputing full-tab or included-session totals', () => {
 const process = (pid: number, domain: string) => ({ pid, parentPid: 1, name: 'p' + pid, domain, memoryBytes: 131072, elapsedSeconds: 9 });
 const response = { sessions: [sessionOutput], tabs: [
  { tabId: 'tab-uuid', memoryBytes: 1048576, elapsedSeconds: 60, processes: [process(42, 'local'), process(43, 'local')] },
  { tabId: 'tab-two', memoryBytes: 2097152, elapsedSeconds: 120, processes: [process(42, 'guest'), process(44, 'guest')] },
 ] };
 const filtered = filterControl(response, { pid: 42 });
 const text = formatControl('processes', filtered);
 expect(filtered.tabs?.map(t => t.processes.map(p => p.pid))).toEqual([[42], [42]]);
 expect(text).toContain('session-uuid  RSS 3.0 MiB  duration 2m 0s');
 expect(text).toContain('+- bash  tab-uuid  RSS 1.0 MiB  duration 1m 0s');
 expect(text).toContain('+- zsh  tab-two  RSS 2.0 MiB  duration 2m 0s');
 expect(text).not.toContain('  PID 43 '); expect(text).not.toContain('  PID 44 ');
 expect(text.match(/  PID 42 /g)).toHaveLength(2);
 expect(response.tabs[0].processes).toHaveLength(2);
});
it('keeps PID trees separated by domain, handles cycles, and renders unknown measurements', () => {
 const proc = (pid: number, parentPid: number, domain = 'local') => ({ pid, parentPid, domain, name: 'p' + pid, memoryBytes: null, elapsedSeconds: null });
 const text = formatControl('processes', { sessions: [{ ...sessionOutput, tabs: sessionOutput.tabs.slice(0, 1) }], tabs: [{
  tabId: 'tab-uuid', memoryBytes: null, elapsedSeconds: null,
  processes: [proc(43, 42), proc(42, 1), proc(43, 1, 'guest'), proc(7, 8), proc(8, 7)],
 }] });
 expect(text).toContain('      +- p42  PID 42  PPID 1  local  RSS n/a  duration n/a');
 expect(text).toContain('        +- p43  PID 43  PPID 42  local');
 expect(text).toContain('      +- p43  PID 43  PPID 1  guest');
 expect(text.match(/  PID 7 /g)).toHaveLength(1); expect(text.match(/  PID 8 /g)).toHaveLength(1);
 expect(formatControl('processes', { sessions: [sessionOutput] })).toContain('Processes n/a');
});
it('escapes terminal controls in names, paths, process domains, errors and picker rows', () => {
 const unsafe = '\x1b[2J\r\n\t\x7f\x9b\u202e';
 expect(escapeTerminal(unsafe)).toBe('\\u001b[2J\\u000d\\u000a\\u0009\\u007f\\u009b\\u202e');
 const bad = { ...sessionOutput, title: unsafe, cwd: unsafe, tabs: [{ ...sessionOutput.tabs[0], title: unsafe, monitoringReason: unsafe }] };
 for (const action of ['list', 'stats', 'processes']) {
  const text = formatControl(action, { sessions: [bad], tabs: [{ tabId: 'tab-uuid', memoryBytes: null, elapsedSeconds: null, processes: [{ pid: 1, parentPid: 0, name: unsafe, domain: unsafe, memoryBytes: null, elapsedSeconds: null }] }] });
  expect(text).not.toMatch(/[\x1b\r\t\x7f\x9b\u202e]/); expect(text).toContain('\\u001b');
 }
 expect(pickerLines([p(unsafe, unsafe, unsafe, 22, unsafe)], 0, false).join('\n')).not.toMatch(/[\x1b\r\t\x7f\x9b\u202e]/);
});
it('quotes remote directories, expands only a leading home prefix, and runs a plain shell',()=>{
 expect(remoteCommand("/srv/it's $(touch /oops)")).toContain("cd -- '/srv/it'\\''s $(touch /oops)'");
 expect(remoteCommand('~/a b')).toContain('cd -- "$HOME"/\'a b\'');expect(remoteCommand('~')).toContain('cd -- "$HOME"');
 expect(remoteCommand('/missing')).toContain('exec "${SHELL:-/bin/sh}" -il');expect(remoteCommand(null)).toBe('exec "${SHELL:-/bin/sh}" -il');
});
