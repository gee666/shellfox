import type { AppError } from '../../shared/contracts';
import { remotePairResponseSchema } from '../../shared/remote-schemas';

export class RemoteAccessError extends Error {
  constructor(readonly detail: AppError) { super(detail.message); }
}
export function requirePair(raw: unknown): void {
  const parsed = remotePairResponseSchema.safeParse(raw);
  if (!parsed.success) throw new RemoteAccessError({ code: 'UNSUPPORTED', message: 'Upgrade the host to use exclusive terminal control.', retryable: false });
  if (!parsed.data.ok) throw new RemoteAccessError(parsed.data.error);
}
export function remoteFailure(error: unknown): AppError {
  return error instanceof RemoteAccessError ? error.detail : { code: 'NATIVE_UNAVAILABLE', message: 'Remote connection failed. Reconnect before retrying an uncertain operation.', retryable: true };
}
export function waitForRemoteRetry(ms: number, signal: AbortSignal): Promise<void> {
  if (signal.aborted) return Promise.resolve();
  return new Promise<void>(resolve => {
    const done = () => { clearTimeout(timer); signal.removeEventListener('abort', done); resolve(); };
    const timer = setTimeout(done, ms); signal.addEventListener('abort', done, { once: true });
  });
}
