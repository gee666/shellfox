/**
 * Deterministic output of a diff-rendering full-screen TUI (modelled on the `pi` agent UI):
 * one full paint, then thousands of tiny cursor-addressed updates wrapped in DEC 2026
 * synchronized output. Only a few rows ever change after the first frame, so any byte tail of
 * the stream — however large — lacks most of the screen. Used by regression tests.
 */
const ESC = '\x1b';
export interface TuiOptions { cols: number; rows: number; alt?: boolean; frames?: number }
const at = (row: number, col: number) => `${ESC}[${row};${col}H`;
const pad = (text: string, width: number) => text.length >= width ? text.slice(0, width) : text + ' '.repeat(width - text.length);

export function tuiFirstFrame({ cols, rows, alt }: TuiOptions): string {
  let out = (alt ? `${ESC}[?1049h` : '') + `${ESC}[?25l${ESC}[?2004h${ESC}[?2026h${ESC}[2J${ESC}[H`;
  for (let row = 1; row <= rows; row++) {
    const label = row === 1 ? 'pi  coding agent' : row === rows ? 'footer: browser connected' : row === rows - 2 ? 'status: working' : `agent ${row} idle`;
    out += at(row, 1) + `${ESC}[38;5;${20 + row}m${ESC}[1m${pad(label, 40)}${ESC}[0m` + `${ESC}[48;2;60;50;40m${pad(`  last action: 00:00:${String(row).padStart(2, '0')}`, cols - 40)}${ESC}[0m`;
  }
  return out + `${ESC}[?2026l${ESC}[${rows};3H${ESC}[?25h`;
}
/** A partial update that touches a single row and a single status cell. */
export function tuiUpdate(options: TuiOptions, frame: number): string {
  const row = 3 + (frame % Math.max(1, options.rows - 6));
  const stamp = `00:${String(Math.floor(frame / 60) % 60).padStart(2, '0')}:${String(frame % 60).padStart(2, '0')}`;
  return `${ESC}[?2026h${ESC}[?25l${at(row, 41)}${ESC}[48;2;60;50;40m${pad(`  last action: ${stamp}`, options.cols - 40)}${ESC}[0m${at(options.rows - 2, 1)}${ESC}[2K${ESC}[35mWorking ${frame}${ESC}[0m${ESC}[?2026l${ESC}[${options.rows};3H${ESC}[?25h`;
}
export function tuiStream(options: TuiOptions): string[] {
  const chunks = [tuiFirstFrame(options)];
  for (let frame = 0; frame < (options.frames ?? 6000); frame++) chunks.push(tuiUpdate(options, frame));
  return chunks;
}
