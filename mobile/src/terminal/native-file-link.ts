import { z } from 'zod';
import { findTerminalFileLinks, parseTerminalFileTarget } from '../../../src/shared/terminal-file-links';
import { positiveDecimalSchema } from '../rust-client/identifiers';

const fileLinkEvent = z.object({
  terminalHandle: positiveDecimalSchema,
  text: z.string().max(8192),
  offset: z.number().int().min(-1).max(8191),
});

// A native tap supplies one visible row and the UTF-16 offset of its cell.
// An explicit OSC 8 target uses -1 and skips visible-text detection.
export function nativeFileLink(event: unknown, terminalHandle: string) {
  const parsed = fileLinkEvent.safeParse(event);
  if (!parsed.success || parsed.data.terminalHandle !== terminalHandle) return null;
  const { text, offset } = parsed.data;
  if (offset === -1) return parseTerminalFileTarget(text);
  const link = findTerminalFileLinks(text).find(value => value.start <= offset && offset < value.end);
  return link ? { path: link.path, line: link.line, column: link.column } : null;
}
