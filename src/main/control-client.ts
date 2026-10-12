import { createConnection } from 'node:net';
import type { ControlRequest, ControlResponse } from './control-types';
import { readControlEndpoint } from './control-endpoint';
import { CONTROL_REQUEST_BYTES, CONTROL_RESPONSE_BYTES, CONTROL_TIMEOUT_MS, ControlFrame, controlReplySchema, controlRequestSchema } from './control-protocol';

/** Errors are thrown so CLI callers cannot mistake failed operations for success. */
export async function requestControl(userData: string, request: ControlRequest): Promise<ControlResponse> {
  const parsed = controlRequestSchema.safeParse(request);
  if (!parsed.success) throw new Error('Invalid control request.');
  let endpoint: Awaited<ReturnType<typeof readControlEndpoint>>;
  try { endpoint = await readControlEndpoint(userData); }
  catch { throw new Error('Shellfox control endpoint is missing or unsafe. Start Shellfox first.'); }
  const payload = JSON.stringify({ version: 1, secret: endpoint.secret, request: parsed.data }) + '\n';
  if (Buffer.byteLength(payload) > CONTROL_REQUEST_BYTES) throw new Error('Control request is too large.');
  return new Promise<ControlResponse>((resolve, reject) => {
    const socket = createConnection({ host: endpoint.host, port: endpoint.port });
    const frame = new ControlFrame(CONTROL_RESPONSE_BYTES);
    let settled = false;
    const finish = (error?: Error, response?: ControlResponse) => {
      if (settled) return;
      settled = true; clearTimeout(deadline); socket.destroy();
      if (error) reject(error); else resolve(response!);
    };
    const deadline = setTimeout(() => finish(new Error('Shellfox control request timed out.')), CONTROL_TIMEOUT_MS);
    socket.on('connect', () => socket.write(payload));
    socket.on('error', () => finish(new Error('Shellfox control connection failed. Start Shellfox first.')));
    socket.on('end', () => finish(new Error('Shellfox control response was incomplete.')));
    socket.on('close', () => finish(new Error('Shellfox control connection closed before replying.')));
    socket.on('data', chunk => {
      try {
        const text = frame.push(chunk);
        if (text === undefined) return;
        const result = controlReplySchema.safeParse(JSON.parse(text));
        if (!result.success) throw new Error('Invalid Shellfox control response.');
        if (!result.data.ok) finish(new Error(result.data.error));
        else finish(undefined, result.data.response);
      } catch (error) { finish(error instanceof Error ? error : new Error('Invalid Shellfox control response.')); }
    });
  });
}
