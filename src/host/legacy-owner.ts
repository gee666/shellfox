import { createConnection } from 'node:net';
import { readControlEndpoint } from '../main/control-endpoint';
import { ControlFrame, controlReplySchema, CONTROL_RESPONSE_BYTES } from '../main/control-protocol';

/** Only absence or an explicitly refused connection proves a legacy owner is gone. */
export async function legacyHostIsRunning(userData: string): Promise<boolean> {
  let endpoint: Awaited<ReturnType<typeof readControlEndpoint>>;
  try { endpoint = await readControlEndpoint(userData); }
  catch (error) { if (error instanceof Error && 'code' in error && error.code === 'ENOENT') return false; throw new Error('Legacy Shellfox endpoint is unsafe or unreadable. Refusing to open its database.', { cause: error }); }
  return new Promise<boolean>((resolve, reject) => {
    const socket = createConnection({ host: endpoint.host, port: endpoint.port });
    const frame = new ControlFrame(CONTROL_RESPONSE_BYTES); let settled = false;
    const finish = (alive: boolean, error?: Error) => { if (settled) return; settled = true; clearTimeout(timer); socket.destroy(); if (error) reject(error); else resolve(alive); };
    const uncertain = () => finish(false, new Error('Legacy Shellfox owner could not be verified. Refusing to open its database.'));
    const timer = setTimeout(uncertain, 3000);
    socket.once('connect', () => socket.write(JSON.stringify({ version: 1, secret: endpoint.secret, request: { action: 'list' } }) + '\n'));
    socket.on('error', (error: NodeJS.ErrnoException) => { if (error.code === 'ECONNREFUSED') finish(false); else uncertain(); });
    socket.on('end', uncertain); socket.on('close', () => { if (!settled) uncertain(); });
    socket.on('data', chunk => {
      try { const text = frame.push(chunk); if (text !== undefined) { controlReplySchema.parse(JSON.parse(text)); finish(true); } }
      catch { uncertain(); }
    });
  });
}
