/** Focus notifications are terminal protocol traffic, not user activity. */
export function isTerminalFocusReport(data: string): boolean {
  return /^(?:\x1b\[[IO])+$/.test(data);
}
