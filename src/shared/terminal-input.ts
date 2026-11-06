/** Focus and button-free pointer motion must reach TUIs without resuming work. */
export function isPassiveTerminalInput(data: string): boolean {
  if (!data) return false;
  let offset = 0;
  while (offset < data.length) {
    const rest = data.slice(offset);
    if (/^\x1b\[[IO]/.test(rest)) { offset += 3; continue; }
    const sgr = /^\x1b\[<(\d+);\d+;\d+M/.exec(rest);
    if (sgr && isHover(Number(sgr[1]))) { offset += sgr[0].length; continue; }
    // Legacy X10 encoding adds 32 to the button and coordinate bytes.
    if (rest.startsWith('\x1b[M') && rest.length >= 6 && isHover(rest.charCodeAt(3) - 32)
      && rest.charCodeAt(4) > 32 && rest.charCodeAt(4) <= 255
      && rest.charCodeAt(5) > 32 && rest.charCodeAt(5) <= 255) { offset += 6; continue; }
    // Mixed frames containing keys, paste, clicks, wheel or drag remain active.
    return false;
  }
  return true;
}

function isHover(button: number): boolean {
  // Motion bit + no button, with only Shift/Alt/Ctrl modifier bits allowed.
  return Number.isInteger(button) && button >= 35 && button <= 63 && (button & 35) === 35;
}
