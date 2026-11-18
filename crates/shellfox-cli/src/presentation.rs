// Presentation cleanup matches src/cli/terminal-presentation.ts.
pub const ENTER: &[u8] = b"\x1b[?1049h\x1b[0m\x1b[2J\x1b[H";
pub const RESET: &[u8] = concat!(
    "\x1b[?2026l\x1b[?2004l\x1b[?1004l",
    "\x1b[?1000l\x1b[?1001l\x1b[?1002l\x1b[?1003l",
    "\x1b[?1005l\x1b[?1006l\x1b[?1015l\x1b[?1016l",
    "\x1b[?1l\x1b>\x1b[?66l",
    "\x1b[?5l\x1b[?6l\x1b[?7h\x1b[4l\x1b[20l",
    "\x1b[0m\x1b[r\x1b[?12l\x1b[?25h",
)
.as_bytes();
pub const LEAVE: &[u8] = b"\x1b[?1049l\x1b[?1047l\x1b[?47l\x1b[0m\x1b[r\x1b[?25h";
