import type { EnvVar } from '../../shared/contracts';
import { envVarsSchemaFor } from '../../shared/schemas';

/** Session overrides win, including Windows' case-insensitive environment names. */
export function terminalEnvironment(inherited: NodeJS.ProcessEnv, variables: EnvVar[], options: { windows: boolean; wsl: boolean; cliBin?: string }): Record<string, string> {
  const env: Record<string, string> = Object.create(null);
  const set = (name: string, value: string) => {
    if (options.windows) for (const key of Object.keys(env)) if (key.toLowerCase() === name.toLowerCase()) delete env[key];
    env[name] = value;
  };
  for (const [key, value] of Object.entries(inherited)) if (typeof value === 'string' && !key.startsWith('SHELLFOX_')) set(key, value);
  const parsed = envVarsSchemaFor(options.windows).parse(variables);
  for (const variable of parsed) set(variable.name, variable.value);
  if (options.cliBin) {
    const name = Object.keys(env).find(key => key.toLowerCase() === 'path') ?? 'PATH', separator = options.windows ? ';' : ':';
    set(name, options.cliBin + separator + (env[name] ?? ''));
  }
  if (options.wsl) {
    const key = Object.keys(env).find(key => key.toLowerCase() === 'wslenv') ?? 'WSLENV';
    const names = new Set(parsed.map(v => v.name.toLowerCase()));
    // Replace flags on session names: transfer these as plain values, not paths/lists.
    const previous = (env[key] ?? '').split(':').filter(entry => entry && !names.has(entry.split('/')[0]!.toLowerCase()));
    set(key, [...previous, ...parsed.map(v => v.name)].join(':'));
  }
  return env;
}
