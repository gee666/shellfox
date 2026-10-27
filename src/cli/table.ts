import { padText, styles, terminalWidth, textWidth, wrapText } from './terminal-text';

/** Keep columns when they fit. Narrow output uses complete labeled records. */
export function formatTable(headers: string[], rows: string[][], color = false, width = terminalWidth(), valueStyle = (value: string) => value): string {
  const widths = headers.map((header, index) => rows.reduce((size, row) => Math.max(size, textWidth(row[index] ?? '')), textWidth(header)));
  const s = styles(color);
  if (widths.reduce((sum, size) => sum + size, 0) + Math.max(0, headers.length - 1) * 2 <= width) {
    const line = (row: string[], heading: boolean) => headers.map((_, index) => {
      const value = row[index] ?? '';
      const text = heading ? s.accent(value) : valueStyle(value);
      return index === headers.length - 1 ? text : padText(text, widths[index]);
    }).join('  ');
    return [line(headers, true), ...rows.map(row => line(row, false))].join('\n');
  }
  if (!rows.length) return wrapText(headers.join('  '), width);
  const labelWidth = Math.max(0, ...headers.map(textWidth));
  return rows.map(row => headers.map((header, index) => {
    const value = row[index] ?? '';
    const prefix = padText(header, labelWidth) + '  ';
    // Put long values on their own lines so a full UUID fits at 40 columns.
    if (textWidth(prefix) + textWidth(value) > width) return wrapText(s.accent(header), width) + '\n' + valueStyle(wrapText(value, width));
    return s.accent(prefix) + valueStyle(value);
  }).join('\n')).join('\n\n');
}
