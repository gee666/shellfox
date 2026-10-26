import { requestControl } from '../main/control-client';
import type { ControlCliIo } from '../main/control-cli';
import { idSchema } from '../shared/schemas';
import { CliUsageError, type GlobalOptions } from './arguments';
export async function resolveSessionId(id: string | undefined, userData: string, options: GlobalOptions,
  marker = process.env.SHELLFOX_TERMINAL_MARKER, request: ControlCliIo['request'] = requestControl): Promise<string> {
  if (id) return id;
  if (options.host && options.host !== 'local') throw new CliUsageError('A remote session ID is required. Local terminal context is never sent to another host.');
  if (!marker || !idSchema.safeParse(marker).success) throw new CliUsageError('No current session. Supply a session UUID or run inside a Shellfox terminal.');
  const inspected = await request(userData, { action: 'inspect', marker, current: true });
  const selected = idSchema.safeParse(inspected.session?.id);
  if (!selected.success) throw new CliUsageError('The current terminal does not identify a live owned session. Supply a session UUID.');
  return selected.data;
}
