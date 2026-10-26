import type { AppError } from '../../../src/shared/contracts';
import type { RemoteMethod, RemoteResponse } from '../../../src/shared/remote-contracts';
import { REMOTE_REQUEST_BYTES, responseSchemas } from '../../../src/shared/remote-schemas';

export type RemotePath = '/v1/pair' | '/v1/request' | '/v1/events' | '/v1/disconnect';

/** Native owns authenticated peer routing, login, discovery and transport deadlines. */
export interface NativeRemoteTransport {
  request(peerId: string, path: RemotePath, body: string): Promise<string>;
}

export class RemoteClientError extends Error {
  constructor(readonly detail: AppError) { super(detail.message); }
}

export function connectionError(error: unknown): AppError {
  return error instanceof RemoteClientError ? error.detail : {
    code: 'NATIVE_UNAVAILABLE',
    message: 'Remote connection failed. Refresh before retrying an uncertain operation.',
    retryable: true,
  };
}

// Keep the method/response relationship without casting a union to a generic result.
const parsers: { [M in RemoteMethod]: { parse(raw: unknown): RemoteResponse<M> } } = responseSchemas;
export function parseResponse<M extends RemoteMethod>(method: M, raw: unknown): RemoteResponse<M> {
  return parsers[method].parse(raw);
}

export function encodeBody(body: unknown): string {
  const json = JSON.stringify(body);
  if (json === undefined || new TextEncoder().encode(json).byteLength > REMOTE_REQUEST_BYTES) {
    throw new RemoteClientError({ code: 'VALIDATION', message: 'Remote request exceeds the encoded size limit.', retryable: false });
  }
  return json;
}

export function parseJson(json: string): unknown {
  if (typeof json !== 'string' || json.length > 8 * 1024 * 1024 || new TextEncoder().encode(json).byteLength > 8 * 1024 * 1024) {
    throw new Error('Remote response exceeds the size limit.');
  }
  return JSON.parse(json);
}

/** Cancel JS work even if an in-flight native request cannot be cancelled. */
export function untilStopped<T>(work: Promise<T>, signal: AbortSignal, timeoutMs = 30_000): Promise<T> {
  return new Promise<T>((resolve, reject) => {
    const stopped = () => finish(() => reject(new Error('Remote request stopped.')));
    const timer = setTimeout(stopped, timeoutMs);
    const finish = (done: () => void) => { clearTimeout(timer); signal.removeEventListener('abort', stopped); done(); };
    signal.addEventListener('abort', stopped, { once: true });
    work.then(value => finish(() => resolve(value)), error => finish(() => reject(error)));
    if (signal.aborted) stopped();
  });
}

export function waitForRetry(ms: number, signal: AbortSignal): Promise<void> {
  return new Promise(resolve => {
    const done = () => { clearTimeout(timer); signal.removeEventListener('abort', done); resolve(); };
    const timer = setTimeout(done, ms);
    signal.addEventListener('abort', done, { once: true });
    if (signal.aborted) done();
  });
}
