import { createServer, type Socket } from 'node:net';
import { randomBytes, timingSafeEqual } from 'node:crypto';
import type { ControlRequest, ControlResponse } from './control-types';
import type { SessionService } from './service';
import { EmbeddedSessionService } from './terminal/service';
import { publishControlEndpoint } from './control-endpoint';
import { CONTROL_REQUEST_BYTES, CONTROL_RESPONSE_BYTES, CONTROL_TIMEOUT_MS, ControlFrame, controlEnvelopeSchema, controlReplySchema, controlRequestSchema } from './control-protocol';

type Service = SessionService | EmbeddedSessionService;
export interface ControlKeepAwake { readonly active: boolean; set(active: boolean): boolean | Promise<boolean> }
export async function handleControlRequest(service: Service, keepAwake: ControlKeepAwake, input: ControlRequest, signal?: AbortSignal): Promise<ControlResponse> {
  const parsed = controlRequestSchema.safeParse(input);
  if (!parsed.success) throw new Error('Invalid control request.');
  const request = parsed.data;
  signal?.throwIfAborted();
  if (request.action === 'keep-alive') return { message: await keepAwake.set(!keepAwake.active) ? 'Keep alive enabled.' : 'Keep alive disabled.' };
  if (request.action === 'list' || request.action === 'stats' || request.action === 'processes') {
    const snapshot = service instanceof EmbeddedSessionService ? await service.getControlSnapshot() : service.getSnapshot();
    signal?.throwIfAborted();
    if (!snapshot.ok) throw new Error(snapshot.error.message);
    let sessions = snapshot.value.sessions;
    if (request.id || request.current) {
      if (!(service instanceof EmbeddedSessionService)) throw new Error('Context selection requires embedded terminals.');
      const selected = await service.controlSession({ action: 'inspect', ...(request.id ? { id: request.id } : { marker: request.marker }) }, signal);
      if (!selected.ok) throw new Error(selected.error.message);
      sessions = sessions.filter(s => s.id === selected.value.session!.id);
      if (!sessions.length) throw new Error('Session not found.');
    }
    if (request.action === 'list') return { sessions };
    if (!(service instanceof EmbeddedSessionService)) throw new Error('Live process statistics require embedded terminals.');
    signal?.throwIfAborted();
    const stats = await service.getSessionStats(sessions.length === 1 ? { sessionId: sessions[0].id } : {});
    if (!stats.ok) throw new Error(stats.error.message);
    const tabs = stats.value.tabs;
    // Keep only the selected sessions' app-owned process details, including settled sessions.
    const selectedTabs = new Set(sessions.flatMap(s => s.tabs.map(t => t.id)));
    return { sessions, tabs: tabs.filter(t => selectedTabs.has(t.tabId)) };
  }
  if (!(service instanceof EmbeddedSessionService)) throw new Error('Session control requires embedded terminals.');
  if (request.action === 'inspect') {
    const refreshed = await service.getControlSnapshot();
    signal?.throwIfAborted();
    if (!refreshed.ok) throw new Error(refreshed.error.message);
  }
  const result = await service.controlSession(request, signal);
  if (!result.ok) throw new Error(result.error.message);
  if (request.action === 'inspect') {
    signal?.throwIfAborted();
    const session = result.value.session;
    if (!session) throw new Error('Session inspection did not return a session.');
    const stats = await service.getSessionStats({ sessionId: session.id });
    if (!stats.ok) throw new Error(stats.error.message);
    const tabs = stats.value.tabs;
    const ids = new Set(session.tabs.map(t => t.id));
    return { ...result.value, tabs: tabs.filter(t => ids.has(t.tabId)) };
  }
  return result.value;
}

export interface ControlServer { dispose(): Promise<void> }
export async function startControlServer(userData: string, service: Service, keepAwake: ControlKeepAwake, guard?: () => void): Promise<ControlServer> {
  const secret = randomBytes(32).toString('hex');
  const sockets = new Set<Socket>();
  let operations = 0;
  let closing = false;
  // A peer may half-close its request stream while still awaiting a reply.
  // Do not let Node auto-close the response stream and abort queued work.
  const server = createServer({ allowHalfOpen: true }, socket => {
    if (closing || sockets.size >= 32) { socket.destroy(); return; }
    sockets.add(socket); socket.setNoDelay(true);
    const controller = new AbortController();
    const deadline = setTimeout(() => { controller.abort(); socket.destroy(); }, CONTROL_TIMEOUT_MS);
    deadline.unref();
    socket.on('error', () => {});
    socket.on('close', () => { controller.abort(); clearTimeout(deadline); sockets.delete(socket); });
    const frame = new ControlFrame(CONTROL_REQUEST_BYTES);
    let received = false;
    socket.on('end', () => { if (!received) socket.destroy(); });
    const reply = (value: unknown) => {
      if (socket.destroyed || closing) return;
      let serialized: string;
      try {
        serialized = JSON.stringify(controlReplySchema.parse(value)) + '\n';
        if (Buffer.byteLength(serialized) > CONTROL_RESPONSE_BYTES) throw new Error('Response too large.');
      } catch { serialized = JSON.stringify({ ok: false, error: 'Control response exceeded limits or was invalid.' }) + '\n'; }
      socket.end(serialized);
    };
    socket.on('data', chunk => {
      if (received || closing) { socket.destroy(); return; }
      let request: ControlRequest;
      try {
        const text = frame.push(chunk);
        if (text === undefined) return;
        received = true;
        const envelope = controlEnvelopeSchema.safeParse(JSON.parse(text));
        if (!envelope.success) throw new Error('Invalid control request.');
        if (!timingSafeEqual(Buffer.from(envelope.data.secret, 'hex'), Buffer.from(secret, 'hex'))) throw new Error('Control authentication failed.');
        guard?.();
        request = envelope.data.request;
      } catch (error) { received = true; reply({ ok: false, error: error instanceof Error ? error.message.slice(0, 1000) : 'Invalid control request.' }); return; }
      // Disconnected stats requests may still be draining the collector. Count
      // that work separately from sockets so reconnects cannot grow its queue.
      if (operations >= 32) { reply({ ok: false, error: 'Shellfox control is busy. Try again later.' }); return; }
      operations++;
      void handleControlRequest(service, keepAwake, request, controller.signal).then(response => reply({ ok: true, response }), error => reply({ ok: false, error: error instanceof Error ? error.message.slice(0, 1000) || 'Control operation failed.' : 'Control operation failed.' })).finally(() => { operations--; });
    });
  });
  // Startup failures reject, later server errors must not crash the manager.
  server.on('error', () => {});
  await new Promise<void>((resolve, reject) => {
    server.once('error', reject);
    server.listen(0, '127.0.0.1', () => { server.removeListener('error', reject); resolve(); });
  });
  const address = server.address();
  if (!address || typeof address === 'string') throw new Error('Control listener unavailable.');
  let removeEndpoint: () => Promise<void>;
  try { removeEndpoint = await publishControlEndpoint(userData, { version: 1, host: '127.0.0.1', port: address.port, secret }); }
  catch (error) { for (const socket of sockets) socket.destroy(); await new Promise<void>(resolve => server.close(() => resolve())); throw error; }
  let disposing: Promise<void> | undefined;
  return { dispose() {
    return disposing ??= (async () => {
      closing = true;
      for (const socket of sockets) socket.destroy();
      await new Promise<void>(resolve => server.close(() => resolve()));
      await removeEndpoint();
    })();
  } };
}
