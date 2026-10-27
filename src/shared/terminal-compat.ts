import { failure } from './contracts';
import { resultSchema, terminalSnapshotSchema } from './schemas';
const legacyAttachment = resultSchema(terminalSnapshotSchema);

/** Metadata can remain usable, but a legacy replay never implies exclusive ownership. */
export function terminalResponseCompatibility(method: string, raw: unknown): unknown {
  if (method === 'attachTerminal') {
    const old = legacyAttachment.safeParse(raw);
    if (old.success && old.data.ok) return failure('UNSUPPORTED', 'Upgrade the host to use exclusive terminal control.');
  }
  return raw;
}
