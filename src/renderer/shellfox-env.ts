import type { EnvVar } from '../shared/contracts';

export interface ShellfoxEnvIssue { line: number; message: string }
export function parseShellfoxEnv(text: string, caseInsensitive: boolean) {
  const env: EnvVar[] = [];
  const issues: ShellfoxEnvIssue[] = [];
  const names = new Set<string>();
  const lines = text.split(/\r?\n/);
  let entries = 0;
  for (const [index, raw] of lines.entries()) {
    const line = index + 1;
    if (raw.includes('\r')) { issues.push({ line, message: 'Values must be single-line with no CR or LF.' }); continue; }
    const source = raw.trim();
    if (!source || source.startsWith('#')) continue;
    const entry = source.replace(/^export[ \t]+/, '');
    const equals = entry.indexOf('=');
    if (equals < 0) { issues.push({ line, message: 'Use NAME=value.' }); continue; }
    if (++entries > 200) { issues.push({ line, message: 'Use at most 200 variables.' }); continue; }
    const name = entry.slice(0, equals).trim();
    let value = entry.slice(equals + 1).trim();
    if (!name || name.length > 256 || /[=:\/\s\u0000]/u.test(name)) {
      issues.push({ line, message: 'Name must be 1 to 256 characters with no whitespace, =, :, / or NUL.' }); continue;
    }
    const quote = value[0];
    if (quote === '"' || quote === "'") {
      if (value.length < 2 || !value.endsWith(quote)) { issues.push({ line, message: 'Values must be single-line; close the quoted value.' }); continue; }
      value = value.slice(1, -1);
    }
    if (value.length > 32767 || /[\r\n\u0000]/.test(value)) {
      issues.push({ line, message: 'Value must be at most 32767 characters with no CR, LF or NUL.' }); continue;
    }
    const key = caseInsensitive ? name.toLowerCase() : name;
    if (names.has(key)) { issues.push({ line, message: 'Duplicate variable name.' }); continue; }
    names.add(key); env.push({ name, value });
  }
  return { env, issues };
}
export function formatShellfoxEnv(env: EnvVar[]) {
  return env.map(({ name, value }) => `${name}=${value.trim() !== value || /^["']/.test(value) || /[\r\n]/.test(value) ? `"${value}"` : value}`).join('\n');
}
