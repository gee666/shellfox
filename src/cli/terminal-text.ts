import { stripVTControlCharacters } from 'node:util';

/** Render untrusted metadata visibly, never as terminal commands. */
export const escapeTerminal = (value: string): string => value.replace(/[\x00-\x1f\x7f-\x9f\u2028\u2029\u202a-\u202e\u2066-\u2069]/g,
  c => '\\u' + c.charCodeAt(0).toString(16).padStart(4, '0'));
export const styles = (color: boolean) => {
  const wrap = (code: string) => (s: string) => color ? `\x1b[${code}m${s}\x1b[0m` : s;
  return { accent: wrap('38;2;236;72;153'), dim: wrap('2'), bold: wrap('1'), red: wrap('31') };
};
const segmenter = new Intl.Segmenter(undefined, { granularity: 'grapheme' });
function graphemeWidth(text: string): number {
  if (/^[\p{Mark}\p{Cf}]*$/u.test(text)) return 0;
  const code = text.codePointAt(0) ?? 0;
  return /\p{Extended_Pictographic}|\p{Regional_Indicator}/u.test(text) ||
    (code >= 0x1100 && (code <= 0x115f || code === 0x2329 || code === 0x232a ||
      (code >= 0x2e80 && code <= 0xa4cf && code !== 0x303f) || (code >= 0xac00 && code <= 0xd7a3) ||
      (code >= 0xf900 && code <= 0xfaff) || (code >= 0xfe10 && code <= 0xfe19) ||
      (code >= 0xfe30 && code <= 0xfe6f) || (code >= 0xff00 && code <= 0xff60) ||
      (code >= 0xffe0 && code <= 0xffe6) || (code >= 0x20000 && code <= 0x3fffd))) ? 2 : 1;
}
export const textWidth = (text: string): number => [...segmenter.segment(stripVTControlCharacters(text))].reduce((width, item) => width + graphemeWidth(item.segment), 0);
export function terminalWidth(output: { columns?: number } = process.stdout): number {
  return output.columns !== undefined && Number.isFinite(output.columns) && output.columns > 0 ? Math.floor(output.columns) : Infinity;
}
/** Wrap by display cells, preserving full values, graphemes and our SGR colors. */
export function wrapText(text: string, width: number): string {
  if (!Number.isFinite(width)) return text;
  width = Math.max(1, Math.floor(width));
  return text.split('\n').flatMap(line => {
    const lines: string[] = []; let current = '', used = 0;
    for (const part of line.split(/(\x1b\[[0-9;]*m)/)) {
      if (/^\x1b\[/.test(part)) { current += part; continue; }
      for (const { segment } of segmenter.segment(part)) {
        const size = graphemeWidth(segment);
        if (used > 0 && used + size > width) { lines.push(current); current = ''; used = 0; }
        current += segment; used += size;
      }
    }
    lines.push(current); return lines;
  }).join('\n');
}
export const padText = (text: string, width: number): string => text + ' '.repeat(Math.max(0, width - textWidth(text)));
