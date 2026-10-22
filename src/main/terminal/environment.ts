import type { EnvVar } from '../../shared/contracts';
import { envVarsSchemaFor } from '../../shared/schemas';

/** Hyperlink opt-ins; session variables can still override them. */
export const HYPERLINK_VARIABLES: readonly (readonly [string, string])[] = [['FORCE_HYPERLINK', '1'], ['PI_HYPERLINKS', '1']];
const MANAGED_NAMES = ['term', 'colorterm', ...HYPERLINK_VARIABLES.map(([name]) => name.toLowerCase())];

/** Session overrides win, except the owned marker, including Windows' case-insensitive names. */
export function terminalEnvironment(inherited: NodeJS.ProcessEnv, variables: EnvVar[], options: { windows: boolean; wsl: boolean; cliBin?: string; marker?: string }): Record<string, string> {
  const env: Record<string, string> = Object.create(null);
  const set = (name: string, value: string) => {
    if (options.windows) for (const key of Object.keys(env)) if (key.toLowerCase() === name.toLowerCase()) delete env[key];
    env[name] = value;
  };
  for (const [key, value] of Object.entries(inherited)) if (typeof value === 'string' && !key.startsWith('SHELLFOX_')) set(key, value);
  // Terminal capabilities are defaults, not inherited host-console limitations.
  // Apply overrides last, including case-insensitive Windows names.
  set('TERM', 'xterm-256color');
  set('COLORTERM', 'truecolor');
  // The embedded xterm.js opens OSC 8 http(s) hyperlinks, but advertises no TERM_PROGRAM
  // that CLI tools recognise. Without these, pi and supports-hyperlinks users (Claude Code,
  // ink, chalk-based CLIs) print "label (long-url)" instead of a clickable label.
  for (const [name, value] of HYPERLINK_VARIABLES) set(name, value);
  const parsed = envVarsSchemaFor(options.windows).parse(variables);
  for (const variable of parsed) {
    const name = options.windows && MANAGED_NAMES.includes(variable.name.toLowerCase()) ? variable.name.toUpperCase() : variable.name;
    set(name, variable.value);
  }
  if (options.cliBin) {
    const name = Object.keys(env).find(key => key.toLowerCase() === 'path') ?? 'PATH', separator = options.windows ? ';' : ':';
    set(name, options.cliBin + separator + (env[name] ?? ''));
  }
  if (options.marker !== undefined) {
    for (const key of Object.keys(env)) if (key.toLowerCase() === 'shellfox_terminal_marker') delete env[key];
    set('SHELLFOX_TERMINAL_MARKER', options.marker);
  }
  // Every Windows shell can launch wsl.exe later. Prepare the same transfer
  // list for local and direct WSL tabs, without exporting the whole host env.
  if (options.windows || options.wsl) {
    const key = Object.keys(env).find(key => key.toLowerCase() === 'wslenv') ?? 'WSLENV';
    const transfer = [...new Set([
      'TERM', 'COLORTERM', ...HYPERLINK_VARIABLES.map(([name]) => name), ...parsed.map(v => v.name),
      ...(options.marker !== undefined ? ['SHELLFOX_TERMINAL_MARKER'] : []),
    ]
      .map(name => Object.keys(env).find(key => key.toLowerCase() === name.toLowerCase()) ?? name))]
      .filter(name => name.toLowerCase() !== 'wslenv');
    const names = new Set(transfer.map(name => name.toLowerCase()));
    // Strip transfer restrictions/transforms from managed values. The marker must
    // cross both directions unchanged; unrelated WSLENV entries keep their flags.
    const previous = (env[key] ?? '').split(':').filter(entry => entry && !names.has(entry.split('/')[0]!.toLowerCase()));
    set('WSLENV', [...previous, ...transfer].join(':'));
  }
  return env;
}
