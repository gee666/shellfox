/** Keep host screen contents off the caller's primary screen and reset remote TUI modes. */
export const ENTER_TERMINAL_SCREEN = '\x1b[?1049h\x1b[0m\x1b[2J\x1b[H';
export const RESET_TERMINAL_MODES = [
  '\x1b[?2026l', // End synchronized output before any cleanup is rendered.
  '\x1b[?2004l', '\x1b[?1004l', // Bracketed paste and focus reports.
  '\x1b[?1000l', '\x1b[?1001l', '\x1b[?1002l', '\x1b[?1003l',
  '\x1b[?1005l', '\x1b[?1006l', '\x1b[?1015l', '\x1b[?1016l',
  '\x1b[?1l', '\x1b>', '\x1b[?66l', // Normal cursor keys and numeric keypad.
  '\x1b[?5l', '\x1b[?6l', '\x1b[?7h', '\x1b[4l', '\x1b[20l',
  '\x1b[0m', '\x1b[r', '\x1b[?12l', '\x1b[?25h',
].join('');
export const LEAVE_TERMINAL_SCREEN = '\x1b[?1049l\x1b[?1047l\x1b[?47l\x1b[0m\x1b[r\x1b[?25h';
export const terminalGrid = (cols: number, rows: number): string => `\x1b[8;${rows};${cols}t`;
