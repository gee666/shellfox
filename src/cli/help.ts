import { terminalWidth, wrapText } from './terminal-text';

const groups: Record<string, { heading: string; rows: Array<[string, string]>; notes?: string[] }> = {
  host: { heading: 'Host', rows: [
    ['host run', 'Run the host in the foreground'], ['host status', 'Show owner, capabilities and uptime'],
    ['host stop', 'Stop the host and its terminals'], ['host service install [--startup]', 'Install same-user OS startup'],
    ['host service uninstall', 'Remove OS startup'], ['host keep-awake [on|off]', 'Read or change sleep prevention'],
  ] },
  session: { heading: 'Session', rows: [
    ['session list [filters]', 'List all sessions'], ['session create [path] [--title name]', 'Create a session'],
    ['session inspect [id]', 'Inspect a session'], ['session rename [id] <title>', 'Rename a session'],
    ['session pin [id] [--off]', 'Pin or unpin a session'], ['session env [id] [--input JSON]', 'Read or replace saved environment'],
    ['session settle [id] [--confirm-active]', 'Set settled status without closing terminals'], ['session unsettle [id]', 'Clear settled status'],
    ['session delete [id]', 'Close owned terminals and delete the session'],
    ['session stats [id] [--all] [filters]', 'Show fresh terminal memory and duration'],
    ['session processes [id] [--all] [filters]', 'Show observed owned processes'],
    ['session focus [id]', 'Focus the session'], ['session refresh [id]', 'Refresh terminal membership'],
    ['session clear-error [id]', 'Clear a saved session error'],
  ], notes: [
    'Aliases: ss, sessions. Deletion accepts rm or -rm within this group.',
    'An explicit UUID or --id wins over current context. Otherwise targeted commands use SHELLFOX_TERMINAL_MARKER.',
    'No marker means an ID is required. Remote targets never inherit a local marker.',
    'list and create need no marker. stats/processes require an ID, current context, or explicit --all.',
    'Filters: --running --idle --no-agents --current --id UUID --name TEXT --path TEXT --tab UUID.',
    'processes also accepts --pid PID. IDs are full UUIDs, never row numbers or prefixes.',
  ] },
  terminals: { heading: 'Terminals', rows: [
    ['terminals list [session-id]', 'List saved terminals'],
    ['terminals create [session-id] [--profile id]', 'Create a terminal in a session'],
    ['terminals rename [session-id] <tab-id> <title>', 'Rename a terminal'],
    ['terminals reorder <session-id> <tab-id>...', 'Save the complete terminal order'],
    ['terminals close <tab-id>', 'Close an owned terminal'], ['terminals attach <tab-id>', 'Attach an existing terminal; Ctrl+] detaches'],
    ['terminals write <tab-id> <text> | --stdin [--take]', 'Write literal input once, without retries'],
  ], notes: ['Alias: tm. There is no tab command.', 'Attach requires an existing terminal UUID and never creates a session or tab.', 'Attach requires a TTY and rejects --json.', 'Only one client views and controls a terminal. Ctrl+T takes it from another client.', 'write refuses an occupied terminal unless --take explicitly switches control.', 'Detaching never stops the shell or host.'] },
  network: { heading: 'Network', rows: [
    ['network status', 'Show Tailscale state and login URL'], ['network enable [--hostname name]', 'Enable remote access'],
    ['network disable', 'Disable remote access'], ['network login [--auth-key-stdin|--auth-key-file path]', 'Log in without secret argv'],
  ] },
  machines: { heading: 'Machines', rows: [['machines list', 'List discovered and remembered machines'], ['machines connect <peer-id>', 'Connect to a machine'], ['machines disconnect <peer-id>', 'Disconnect'], ['machines forget <peer-id>', 'Forget a saved machine']] },
  approvals: { heading: 'Approvals', rows: [['approvals list', 'List approvals and pending requests'], ['approvals approve <peer-id>', 'Grant access'], ['approvals revoke <peer-id>', 'Remove approval'], ['approvals reject <peer-id>', 'Reject a pending request']] },
  settings: { heading: 'Settings', rows: [['settings read', 'Read current settings'], ['settings update --input JSON', 'Merge and validate settings'], ['profiles list', 'List terminal profiles'], ['path home|resolve|complete|create [path]', 'Use host directory helpers']] },
  commands: { heading: 'Commands', rows: [['commands --register <name> <script-path>', 'Register a script reference'], ['commands <name> [args...]', 'Run a script with literal arguments'], ['commands --list', 'List saved script commands'], ['commands --remove <name>', 'Remove a reference'], ['commands --rename <old> <new>', 'Rename a reference']], notes: ['Alias: cmd. Administration accepts trailing global flags; everything after a script name is opaque.', 'Aliases: --run <name> for execution, --ls for --list, --rm and --unregister for --remove.'] },
  ssh: { heading: 'SSH', rows: [['ssh [--profile name]', 'Connect to a saved SSH profile']], notes: ['Alias: -p for --profile. A positional profile name is also accepted.'] },
  automation: { heading: 'Automation', rows: [['run [--cwd PATH] [--detach] -- EXE [ARG...]', 'Run with ordinary process pipes in the local host'], ['runs list', 'List retained automation runs'], ['runs show <run-id>', 'Open a read-only terminal view without changing I/O'], ['runs stop <run-id> --yes', 'Stop a run and its process tree'], ['api <ManagerMethod> [--input JSON]', 'Call a validated manager method'], ['remote api <method> [--input JSON]', 'Call local remote management'], ['completion <powershell|bash|zsh>', 'Print shell completion code']], notes: ['Foreground run preserves stdout, stderr, stdin and exit codes. Arguments after -- are literal; no shell expansion occurs.', 'run accepts --title NAME and --request-id UUID to identify a subagent without changing its output.', 'Use --detach --json to return a run ID immediately. Detached stdin is closed. Runs last only while their host runs.', 'runs show creates a terminal view visible to desktop and mobile clients. Its replay is bounded; it is not a full transcript.', '--input accepts inline JSON, @file.json, or - for stdin. Human-readable output is the default.'] },
  update: { heading: 'Update', rows: [['update [--check]', 'Update this local installation']], notes: ['Headless: --check reports the latest matching stable release; update installs a verified version for future launches.', 'Headless updates accept --json and never stop hosts or change PATH/service paths. Keep the original launcher and restart the host explicitly; stopping a host ends its running commands.', 'Desktop: use the installed platform launcher or the in-app updater. CLI installers support Windows x64 Squirrel, Linux x64/ARM64 .deb and macOS x64/ARM64 .app. Portable Windows/Linux copies and source checkouts need manual updates.', 'Desktop update rejects --json; use api getUpdateStatus or api downloadUpdate for JSON automation. update is local-only; remote --host targets are rejected, not forwarded.'] },
  application: { heading: 'Application', rows: [['version', 'Print the installed version'], ['help [group]', 'Show command help']], notes: ['Aliases: --version, -v for version; --help, -h for help.'] },
};
const aliases: Record<string, string> = { ss: 'session', sessions: 'session', tm: 'terminals', cmd: 'commands', profiles: 'settings', path: 'settings', api: 'automation', run: 'automation', runs: 'automation', remote: 'automation', completion: 'automation', version: 'application' };
export function knownHelpTopic(topic: string): boolean { return Object.hasOwn(groups, aliases[topic] ?? topic); }
export function cliHelp(version = '', topic?: string, color = false, columns = terminalWidth()): string {
  const key = topic ? aliases[topic.split(' ')[0]] ?? topic.split(' ')[0] : undefined;
  const selected = key ? groups[key] ? [groups[key]] : [] : Object.values(groups);
  const accent = (text: string) => color ? '\x1b[38;2;236;72;153m' + text + '\x1b[0m' : text;
  const lines = [accent('shellfox') + (version ? ' ' + version : ''), ''];
  for (const group of selected) {
    lines.push(accent(group.heading));
    const width = Math.max(...group.rows.map(([command]) => command.length));
    for (const [command, description] of group.rows) {
      if (width + description.length + 4 <= columns) lines.push('  ' + command.padEnd(width) + '  ' + description);
      else lines.push('  ' + command, '    ' + description);
    }
    for (const note of group.notes ?? []) if (topic || /^Alias(?:es)?:/.test(note)) lines.push('  ' + note);
    lines.push('');
  }
  lines.push('Global options', '  --host <local|peer-id|hostname>  Select the host', '  --user-data <path>              Select the host data directory', '  --json                         Structured output and errors; never prompt', '  --yes                          Confirm destructive operations', '', 'Destructive noninteractive operations require --yes. NO_COLOR disables color.', 'Run shellfox help <group> for context rules and options.', '');
  return wrapText(lines.join('\n'), columns);
}
