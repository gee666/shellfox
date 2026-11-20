import { z } from 'zod';
import { parseTerminalFileTarget } from '../../../src/shared/terminal-file-links';
import { positiveDecimalSchema } from '../rust-client/identifiers';

const fileLinkEvent = z.object({
  terminalHandle: positiveDecimalSchema,
  text: z.string().max(8192),
});

// Native resolves the target at the tapped cell. Validate it before opening Files.
export function nativeFileLink(event: unknown, terminalHandle: string) {
  const parsed = fileLinkEvent.safeParse(event);
  if (!parsed.success || parsed.data.terminalHandle !== terminalHandle) return null;
  return parseTerminalFileTarget(parsed.data.text);
}
