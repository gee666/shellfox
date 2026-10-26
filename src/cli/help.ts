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
    ['terminals close <tab-id>', 'Close an owned terminal'], ['terminals attach <tab-id>', 'Attach interactively; Ctrl+] detaches'],
    ['terminals write <tab-id> <text> | --stdin', 'Write literal input once, without retries'],
  ], notes: ['Alias: tm. There is no tab command.', 'Attach requires a TTY and rejects --json. Detaching never stops the shell or host.'] },
  network: { heading: 'Network', rows: [
    ['network status', 'Show Tailscale state and login URL'], ['network enable [--hostname name]', 'Enable remote access'],
    ['network disable', 'Disable remote access'], ['network login [--auth-key-stdin|--auth-key-file path]', 'Log in without secret argv'],
  ] },
  machines: { heading: 'Machines', rows: [['machines list', 'List discovered and remembered machines'], ['machines connect <peer-id>', 'Connect to a machine'], ['machines disconnect <peer-id>', 'Disconnect'], ['machines forget <peer-id>', 'Forget a saved machine']] },
  approvals: { heading: 'Approvals', rows: [['approvals list', 'List approvals and pending requests'], ['approvals approve <peer-id>', 'Grant access'], ['approvals revoke <peer-id>', 'Remove approval'], ['approvals reject <peer-id>', 'Reject a pending request']] },
  settings: { heading: 'Settings', rows: [['settings read', 'Read current settings'], ['settings update --input JSON', 'Merge and validate settings'], ['profiles list', 'List terminal profiles'], ['path home|resolve|complete|create [path]', 'Use host directory helpers']] },
  commands: { heading: 'Commands', rows: [['commands --register <name> <script-path>', 'Register a script reference'], ['commands <name> [args...]', 'Run a script with literal arguments'], ['commands --list', 'List saved script commands'], ['commands --remove <name>', 'Remove a reference'], ['commands --rename <old> <new>', 'Rename a reference']], notes: ['Alias: cmd. Administration accepts trailing global flags; everything after a script name is opaque.'] },
  ssh: { heading: 'SSH', rows: [['ssh [--profile name]', 'Connect to a saved SSH profile']] },
  automation: { heading: 'Automation', rows: [['api <ManagerMethod> [--input JSON]', 'Call a validated manager method'], ['remote api <method> [--input JSON]', 'Call local remote management'], ['completion <powershell|bash|zsh>', 'Print shell completion code']], notes: ['--input accepts inline JSON, @file.json, or - for stdin. Human-readable output is the default.'] },
  application: { heading: 'Application', rows: [['update [--check]', 'Use the desktop platform updater'], ['version', 'Print the installed version'], ['help [group]', 'Show command help']] },
};
const aliases: Record<string, string> = { ss: 'session', sessions: 'session', tm: 'terminals', cmd: 'commands', profiles: 'settings', path: 'settings', api: 'automation', remote: 'automation', completion: 'automation', update: 'application', version: 'application' };
export function knownHelpTopic(topic: string): boolean { return Object.hasOwn(groups, aliases[topic] ?? topic); }
export function cliHelp(version = '', topic?: string, color = false): string {
  const key = topic ? aliases[topic.split(' ')[0]] ?? topic.split(' ')[0] : undefined;
  const selected = key ? groups[key] ? [groups[key]] : [] : Object.values(groups);
  const accent = (text: string) => color ? '\x1b[38;2;236;72;153m' + text + '\x1b[0m' : text;
  const lines = [accent('shellfox') + (version ? ' ' + version : ''), ''];
  for (const group of selected) {
    lines.push(accent(group.heading));
    const width = Math.max(...group.rows.map(([command]) => command.length));
    for (const [command, description] of group.rows) lines.push('  ' + command.padEnd(width) + '  ' + description);
    if (topic) for (const note of group.notes ?? []) lines.push('  ' + note);
    lines.push('');
  }
  lines.push('Global options', '  --host <local|peer-id|hostname>  Select the host', '  --user-data <path>              Select the host data directory', '  --json                         Structured output and errors; never prompt', '  --yes                          Confirm destructive operations', '', 'Destructive noninteractive operations require --yes. NO_COLOR disables color.', 'Run shellfox help <group> for context rules and options.', '');
  return lines.join('\n');
}
