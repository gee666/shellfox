import { EventEmitter } from 'node:events';
import { PassThrough, Readable } from 'node:stream';
import path from 'node:path';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { hostReplySchema } from '../shared/host-schemas';
import type { HostEndpoint, HostMethod, HostRequest } from '../shared/host-contracts';
import { failure } from '../shared/contracts';
import type { HostSessionService } from './operations';
import { startHostRpc, type HostRpc } from './rpc';

const mocks = vi.hoisted(() => ({
  server: vi.fn(), publish: vi.fn(), remove: vi.fn(), manager: vi.fn(),
  launchAutomation: vi.fn(), requestAutomationStop: vi.fn(), disposeClients: vi.fn(), unsubscribe: vi.fn(),
}));
const clientId = 'ca2a7cf5-91f4-45e6-98b0-693dbb6226aa';
vi.mock('node:http', () => ({ createServer: mocks.server }));
vi.mock('../main/control-endpoint', () => ({ publishPrivateEndpoint: mocks.publish }));
vi.mock('./operations', () => ({ managerOperation: mocks.manager }));
vi.mock('./automation-launch', () => ({ launchAutomation: mocks.launchAutomation, requestAutomationStop: mocks.requestAutomationStop }));
vi.mock('./rpc-clients', () => ({ RpcClients: class {
  open = () => ({ id: clientId });
  get = () => ({ id: clientId, busy: 0 });
  dispose = mocks.disposeClients;
} }));

function gate() {
  let resolve = () => {};
  const promise = new Promise<void>(done => { resolve = done; });
  return { promise, resolve };
}

class Socket extends EventEmitter {
  destroyed = false;
  destroy() {
    if (this.destroyed) return;
    this.destroyed = true;
    this.emit('close');
  }
}
class Reply extends EventEmitter {
  status?: number;
  body?: ReturnType<typeof hostReplySchema.parse>;
  private readonly done = gate();
  readonly finished = this.done.promise;
  constructor(readonly socket: Socket) {
    super();
    socket.once('close', () => { this.emit('close'); this.done.resolve(); });
  }
  get destroyed() { return this.socket.destroyed; }
  writeHead(status: number) { this.status = status; }
  end(text?: string) {
    if (text !== undefined) this.body = hostReplySchema.parse(JSON.parse(text));
    this.emit('finish'); this.socket.destroy();
  }
  destroy() { this.socket.destroy(); }
}
type Request = Readable & { socket: Socket; method: string; url: string; headers: { authorization: string } };

// A model of the HTTP listener and socket lifetimes. No socket, file, or child is opened.
class Listener extends EventEmitter {
  private readonly sockets = new Set<Socket>();
  private onClosed?: (error?: Error) => void;
  closeCalls = 0;
  listening = false;
  constructor(private readonly receive: (request: Request, reply: Reply) => void) { super(); }
  address() { return { port: 12345 }; }
  listen(_port: number, _host: string, ready: () => void) { this.listening = true; ready(); }
  close(done: (error?: Error) => void) {
    this.closeCalls++; this.listening = false; this.onClosed = done; this.maybeClosed();
  }
  private maybeClosed() {
    if (this.sockets.size || !this.onClosed) return;
    const closed = this.onClosed; this.onClosed = undefined; closed();
  }
  closeAllConnections() { for (const socket of this.sockets) socket.destroy(); }
  send<M extends HostMethod>(endpoint: HostEndpoint, method: M, payload: HostRequest<M>): Reply {
    if (!this.listening) throw new Error('Listener is closed');
    const socket = new Socket();
    this.sockets.add(socket);
    socket.once('close', () => { this.sockets.delete(socket); this.maybeClosed(); });
    this.emit('connection', socket);
    const body = { version: 1, requestId: 'f390cfff-b458-4a5a-b79b-ff44b86f6158', clientId, method, payload };
    const request = Object.assign(Readable.from([Buffer.from(JSON.stringify(body))]), {
      socket, method: 'POST', url: '/v1/request', headers: { authorization: `Bearer ${endpoint.secret}` },
    });
    const reply = new Reply(socket);
    this.receive(request, reply);
    return reply;
  }
}

// Only subscribe is used by the server in these cases; any accidental service call fails.
function unused(): never { throw new Error('Unexpected fixture service call'); }
const service: HostSessionService = {
  subscribe: () => mocks.unsubscribe, createTerminalView: unused, getSessionStats: unused, getSnapshot: unused,
  getTerminalProfiles: unused, closeTab: unused, createSession: unused, addTab: unused, activateSession: unused,
  focusSession: unused, refreshSessionMembership: unused, prepareSessionRegistration: unused, renameSession: unused,
  renameTab: unused, reorderTabs: unused, setSessionPinned: unused, settleSession: unused, unsettleSession: unused,
  deleteSession: unused, clearSessionError: unused, retryTab: unused, setSessionEnv: unused, setTerminalEnv: unused,
  saveSettings: unused, setExplorerIntegration: unused, setCliIntegration: unused, setRemoteVisibleSessions: unused,
  clearRemoteVisibleSessions: unused,
};
const timeout = 25;
async function fixture(stop?: (rpc: HostRpc) => Promise<void>, drainTimeoutMs = timeout) {
  let listener: Listener | undefined, endpoint: HostEndpoint | undefined;
  mocks.server.mockImplementation((_options, receive: ConstructorParameters<typeof Listener>[0]) => (listener = new Listener(receive)));
  mocks.publish.mockImplementation(async (_root, _file, _schema, data: HostEndpoint) => { endpoint = data; return mocks.remove; });
  const shutdown = vi.fn(async (): Promise<void> => { await (stop ? stop(rpc) : rpc.close()); });
  const rpc: HostRpc = await startHostRpc({ userData: 'fixture-only', service, shutdown, mode: 'desktop', drainTimeoutMs });
  if (!listener || !endpoint) throw new Error('Missing listener or endpoint');
  const server: Listener = listener, target: HostEndpoint = endpoint;
  return { rpc, server, shutdown, send: <M extends HostMethod>(method: M, payload: HostRequest<M>) => server.send(target, method, payload) };
}
const unsupported = () => failure('UNSUPPORTED', 'Fixture action completed.');
function pendingAction() {
  const entered = gate(), exit = gate();
  mocks.manager.mockImplementationOnce(async () => { entered.resolve(); await exit.promise; return unsupported(); });
  return { entered, exit };
}

class Worker extends EventEmitter {
  readonly pid = 921;
  readonly stdout = new PassThrough();
  readonly stderr = new PassThrough();
  readonly writing = gate();
  readonly stdin = Object.assign(new EventEmitter(), {
    write: vi.fn(() => { this.writing.resolve(); return true; }), end: vi.fn(),
  });
}
const runId = '50c605bf-5d1a-4705-8e6b-ac88023481bd';
async function startWorker(f: Awaited<ReturnType<typeof fixture>>) {
  const worker = new Worker();
  mocks.launchAutomation.mockReturnValueOnce({ child: worker, ready: Promise.resolve(), outcome: () => undefined });
  const start = f.send('automation.start', { requestId: runId, executable: 'fixture-only', args: [], cwd: path.resolve('tmp'), env: {}, detached: false });
  await start.finished;
  expect(start.body).toMatchObject({ ok: true, value: { id: runId, state: 'running' } });
  return worker;
}

beforeEach(() => {
  vi.resetAllMocks();
  vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout', 'setInterval', 'clearInterval'] });
  mocks.launchAutomation.mockImplementation(() => { throw new Error('Unexpected fixture launch'); });
  mocks.disposeClients.mockResolvedValue(undefined);
  mocks.remove.mockResolvedValue(undefined);
});
afterEach(() => vi.useRealTimers());

describe('Host RPC drain', () => {
  it('rejects a timed-out drain and joins the same disconnected action on retry', async () => {
    const f = await fixture(), action = pendingAction();
    const request = f.send('chooseDirectory', {}); await action.entered.promise;
    const first = f.rpc.close(); expect(f.rpc.close()).toBe(first);
    const failed = expect(first).rejects.toThrow('pending requests');
    await vi.advanceTimersByTimeAsync(timeout); await failed; await request.finished;
    expect(f.server.listening).toBe(false);
    expect(mocks.requestAutomationStop).not.toHaveBeenCalled();
    expect(mocks.disposeClients).not.toHaveBeenCalled();
    expect(mocks.remove).not.toHaveBeenCalled();

    const retry = f.rpc.close(); action.exit.resolve(); await retry; await f.rpc.close();
    expect(f.server.closeCalls).toBe(1);
    expect(mocks.manager).toHaveBeenCalledTimes(1);
    expect(mocks.requestAutomationStop).not.toHaveBeenCalled();
    expect(mocks.disposeClients).toHaveBeenCalledTimes(1);
    expect(mocks.remove).toHaveBeenCalledTimes(1);
  });

  it('does not dispatch a request whose body arrives after the close fence', async () => {
    const f = await fixture();
    const request = f.send('chooseDirectory', {});
    await f.rpc.close(); await request.finished;
    expect(mocks.manager).not.toHaveBeenCalled();
    expect(mocks.remove).toHaveBeenCalledTimes(1);
  });

  it('refuses a waiting Stop after drain timeout and leaves downstream cleanup for the owner retry', async () => {
    const completed = vi.fn(), stopEntered = gate();
    const f = await fixture(async rpc => { stopEntered.resolve(); await rpc.close(); completed(); });
    const action = pendingAction(); f.send('chooseDirectory', {}); await action.entered.promise;
    const stop = f.send('host.stop', { wait: true }); await stopEntered.promise;
    await vi.advanceTimersByTimeAsync(timeout); await stop.finished;
    expect(stop.body).toMatchObject({ ok: false, error: expect.stringContaining('pending requests') });
    expect(completed).not.toHaveBeenCalled();
    action.exit.resolve(); await f.shutdown();
    expect(completed).toHaveBeenCalledTimes(1);
    expect(f.server.closeCalls).toBe(1);
  });

  it('does not wait for a Stop reply that depends on the shutdown result', async () => {
    const closed = gate(), downstream = gate();
    const f = await fixture(async rpc => { await rpc.close(); closed.resolve(); await downstream.promise; });
    const stop = f.send('host.stop', { wait: true }); await closed.promise;
    let flushed = false;
    const replies = f.rpc.waitForStopReplies().then(() => { flushed = true; });
    expect(stop.body).toBeUndefined(); expect(flushed).toBe(false);
    downstream.resolve(); await stop.finished; await replies;
    expect(stop.body).toMatchObject({ ok: true, value: { stopped: true } });
  });

  it('starts original worker shutdown while an admitted write is stuck and still waits for close', async () => {
    const f = await fixture(), worker = await startWorker(f), requested = gate();
    mocks.requestAutomationStop.mockImplementationOnce(() => requested.resolve());
    const write = f.send('automation.write', { id: runId, data: 'YQ==', eof: true });
    await worker.writing.promise;
    const stop = f.send('host.stop', { wait: true });
    await requested.promise;
    expect(write.body).toBeUndefined();
    expect(stop.body).toBeUndefined();
    expect(mocks.disposeClients).not.toHaveBeenCalled();
    expect(mocks.requestAutomationStop).toHaveBeenCalledExactlyOnceWith(worker);
    worker.emit('close', 0, null);
    await stop.finished;
    expect(worker.stdin.end).not.toHaveBeenCalled();
    expect(stop.body).toMatchObject({ ok: true, value: { stopped: true } });
  });

  it('drains buffered stdout and stderr at capture credit without re-pausing either pipe', async () => {
    const f = await fixture(), worker = await startWorker(f), requested = gate(), filled = gate();
    const stdoutPause = vi.spyOn(worker.stdout, 'pause'), stderrPause = vi.spyOn(worker.stderr, 'pause');
    worker.stdout.once('data', filled.resolve);
    worker.stdout.write(Buffer.alloc(1024 * 1024, 0x61));
    await filled.promise;
    expect(stdoutPause).toHaveBeenCalledTimes(1);
    expect(stderrPause).toHaveBeenCalledTimes(1);

    const stdoutTail = vi.fn(), stderrTail = vi.fn();
    worker.stdout.once('data', stdoutTail); worker.stderr.once('data', stderrTail);
    worker.stdout.write('stdout tail'); worker.stderr.write('stderr tail');
    expect(worker.stdout.readableLength).toBeGreaterThan(0);
    expect(worker.stderr.readableLength).toBeGreaterThan(0);

    mocks.requestAutomationStop.mockImplementationOnce(() => requested.resolve());
    const stop = f.send('automation.stop', { id: runId });
    await requested.promise;
    await new Promise<void>(resolve => setImmediate(resolve));
    expect(stdoutPause).toHaveBeenCalledTimes(1);
    expect(stderrPause).toHaveBeenCalledTimes(1);
    expect(stdoutTail).toHaveBeenCalledExactlyOnceWith(Buffer.from('stdout tail'));
    expect(stderrTail).toHaveBeenCalledExactlyOnceWith(Buffer.from('stderr tail'));
    expect(stop.body).toBeUndefined();

    // The original MiB is 32 frames. No new frame was retained above credit.
    const read = f.send('automation.read', { id: runId, after: 32 });
    await read.finished;
    expect(read.body).toMatchObject({ ok: true, value: { frames: [] } });
    worker.emit('close', 0, null);
    await stop.finished;
    expect(stop.body).toMatchObject({ ok: true, value: { stopped: true } });
    await f.rpc.close();
  });

  it('re-observes the same worker after the automation deadline without treating exit alone as close', async () => {
    const f = await fixture(undefined, 15_000), worker = await startWorker(f);
    const first = expect(f.rpc.close()).rejects.toThrow('Automation process exit was not confirmed.');
    await vi.advanceTimersByTimeAsync(10_000); await first;
    expect(mocks.remove).not.toHaveBeenCalled();
    worker.emit('exit', 0, null);
    const second = expect(f.rpc.close()).rejects.toThrow('Automation process exit was not confirmed.');
    await vi.advanceTimersByTimeAsync(10_000); await second;
    expect(mocks.remove).not.toHaveBeenCalled();
    worker.emit('close', 0, null);
    await f.rpc.close();
    expect(mocks.requestAutomationStop).toHaveBeenCalledExactlyOnceWith(worker);
    expect(mocks.remove).toHaveBeenCalledTimes(1);
  });

  it('never reissues a shutdown request that threw, but can join the original late close', async () => {
    const f = await fixture(), worker = await startWorker(f);
    mocks.requestAutomationStop.mockImplementationOnce(() => { throw new Error('Termination request inconclusive.'); });
    await expect(f.rpc.close()).rejects.toThrow('Termination request inconclusive.');
    const retry = expect(f.rpc.close()).rejects.toThrow('automation exit');
    await vi.advanceTimersByTimeAsync(timeout); await retry;
    expect(mocks.remove).not.toHaveBeenCalled();
    worker.emit('close', null, 'SIGKILL');
    await f.rpc.close();
    expect(mocks.requestAutomationStop).toHaveBeenCalledExactlyOnceWith(worker);
    expect(mocks.remove).toHaveBeenCalledTimes(1);
  });

  it('keeps original in-flight client cleanup on timeout rather than invoking it twice', async () => {
    const f = await fixture(), entered = gate(), exit = gate();
    mocks.disposeClients.mockImplementationOnce(async () => { entered.resolve(); await exit.promise; });
    const failed = expect(f.rpc.close()).rejects.toThrow('listener and client cleanup');
    await entered.promise; await vi.advanceTimersByTimeAsync(timeout); await failed;
    expect(mocks.remove).not.toHaveBeenCalled();
    const retry = f.rpc.close(); exit.resolve(); await retry;
    expect(mocks.disposeClients).toHaveBeenCalledTimes(1);
    expect(mocks.remove).toHaveBeenCalledTimes(1);
  });

  it('keeps a rejected cleanup failed without replaying it or removing the endpoint', async () => {
    const f = await fixture();
    mocks.disposeClients.mockRejectedValueOnce(new Error('Cleanup disposition unknown.'));
    await expect(f.rpc.close()).rejects.toThrow('Cleanup disposition unknown.');
    await expect(f.rpc.close()).rejects.toThrow('Cleanup disposition unknown.');
    expect(mocks.disposeClients).toHaveBeenCalledTimes(1);
    expect(mocks.remove).not.toHaveBeenCalled();
  });

  it('treats a pending update as ordinary work when another caller starts close', async () => {
    const f = await fixture(), action = pendingAction();
    f.send('installUpdate', { confirmCloseTerminals: true }); await action.entered.promise;
    const failed = expect(f.rpc.close()).rejects.toThrow('pending requests');
    await vi.advanceTimersByTimeAsync(timeout); await failed;
    expect(mocks.remove).not.toHaveBeenCalled();
    action.exit.resolve(); await f.rpc.close();
  });

  it('lets the initiating update close without itself but does not exempt it for an outside caller', async () => {
    const f = await fixture(), closing = gate(), drained = gate(), exit = gate();
    mocks.manager.mockImplementationOnce(async () => {
      const task = f.rpc.close(); closing.resolve(); await task; drained.resolve();
      await exit.promise; return unsupported();
    });
    f.send('installUpdate', { confirmCloseTerminals: true }); await closing.promise;
    const failed = expect(f.rpc.close()).rejects.toThrow('original update request');
    await drained.promise; await vi.advanceTimersByTimeAsync(timeout); await failed;
    exit.resolve(); await f.rpc.close();
    expect(f.server.closeCalls).toBe(1);
    expect(mocks.remove).toHaveBeenCalledTimes(1);
  });
});
