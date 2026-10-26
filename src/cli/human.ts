import { escapeTerminal, styles } from '../main/ssh/presentation';
export const colorEnabled = (output: { isTTY?: boolean } = process.stdout, env: NodeJS.ProcessEnv = process.env): boolean =>
  Boolean(output.isTTY && env.NO_COLOR === undefined && env.TERM !== 'dumb');
const record = (value: unknown): value is Record<string, unknown> => value !== null && typeof value === 'object' && !Array.isArray(value);
const label = (key: string) => key.replace(/([a-z])([A-Z])/g, '$1 $2').replaceAll('_', ' ');
const scalar = (value: unknown): boolean => value === null || ['string', 'number', 'boolean', 'undefined'].includes(typeof value);
function cell(value: unknown): string {
  if (value === null || value === undefined) return 'none';
  if (typeof value === 'boolean') return value ? 'yes' : 'no';
  if (typeof value === 'string' || typeof value === 'number') return escapeTerminal(String(value));
  if (Array.isArray(value)) return String(value.length);
  return '';
}
function status(value: string, color: boolean): string {
  const s = styles(color);
  if (/^(error|failed|unavailable|closed|disabled)$/i.test(value)) return s.red(value);
  if (/^(online|connected|open|running|yes|enabled)$/i.test(value)) return color ? '\x1b[32m' + value + '\x1b[0m' : value;
  return value;
}
export function humanTable(headers: string[], rows: string[][], color = false): string {
  const widths = headers.map((header, index) => Math.max(header.length, ...rows.map(row => (row[index] ?? '').length)));
  const line = (row: string[], heading: boolean) => row.map((value, index) => {
    const padded = index === row.length - 1 ? value : value.padEnd(widths[index]);
    return heading ? styles(color).accent(padded) : status(padded.trimEnd(), color) + padded.slice(padded.trimEnd().length);
  }).join('  ');
  return [line(headers, true), ...rows.map(row => line(row, false))].join('\n');
}
function arrayText(values: unknown[], color: boolean): string {
  if (!values.length) return 'None.';
  if (values.every(scalar)) return values.map(value => cell(value)).join('\n');
  if (values.every(record)) {
    if (values.every(value => 'title' in value && 'cwd' in value && 'tabs' in value)) {
      return humanTable(['NAME', 'ID', 'STATUS', 'TERMINALS', 'PATH'], values.map(value => [cell(value.title), cell(value.id), cell(value.status), cell(value.tabs), cell(value.cwd)]), color);
    }
    if (values.every(value => 'sessionId' in value && 'lifecycle' in value)) {
      return humanTable(['NAME', 'ID', 'SESSION', 'STATE', 'PROFILE', 'PATH'], values.map(value => [cell(value.title), cell(value.id), cell(value.sessionId), cell(value.lifecycle), cell(value.profileId), cell(value.cwd)]), color);
    }
    if (values.every(value => 'hostname' in value && 'connection' in value)) {
      return humanTable(['MACHINE', 'ID', 'ONLINE', 'CONNECTION', 'APPROVED', 'ERROR'], values.map(value => [cell(value.hostname), cell(value.id), cell(value.online), cell(value.connection), cell(value.approved), record(value.lastError) ? cell(value.lastError.message) : 'none']), color);
    }
    const keys = [...new Set(values.flatMap(value => Object.keys(value)))].filter(key => values.every(value => scalar(value[key]))).slice(0, 8);
    if (keys.length) {
      const table = humanTable(keys.map(key => label(key).toUpperCase()), values.map(value => keys.map(key => cell(value[key]))), color);
      const details = values.flatMap((value, index) => {
        const nested = Object.fromEntries(Object.entries(value).filter(([, item]) => !scalar(item)));
        return Object.keys(nested).length ? [styles(color).accent(cell(value.id ?? value.name ?? index + 1)) + '\n' + indent(renderHuman(nested, color))] : [];
      });
      return [table, ...details].join('\n');
    }
  }
  return values.map((value, index) => styles(color).accent(String(index + 1)) + '\n' + indent(renderHuman(value, color))).join('\n');
}
const indent = (text: string) => text.split('\n').map(line => '  ' + line).join('\n');
/** API objects get readable fields and tables, never an implicit JSON dump. */
export function renderHuman(value: unknown, color = false): string {
  if (Array.isArray(value)) return arrayText(value, color);
  if (!record(value)) return status(cell(value), color);
  const entries = Object.entries(value);
  if (!entries.length) return 'Done.';
  const fields = entries.filter(([, item]) => scalar(item));
  const width = Math.max(0, ...fields.map(([key]) => label(key).length));
  const lines = fields.map(([key, item]) => styles(color).dim(label(key).padEnd(width)) + '  ' + status(cell(item), color));
  for (const [key, item] of entries.filter(([, item]) => !scalar(item))) {
    lines.push(styles(color).accent(label(key)) + '\n' + indent(renderHuman(item, color)));
  }
  return lines.join('\n');
}
