import type { AppError, ManagerApi, Result, TerminalApi } from '../shared/contracts';

export function getManagerApi(): ManagerApi | undefined {
  return (window as Window & { shellfox?: ManagerApi }).shellfox;
}

export function getTerminalApi(api: ManagerApi): TerminalApi | null {
  const methods: (keyof TerminalApi)[] = ['getTerminalProfiles', 'attachTerminal', 'writeTerminal', 'resizeTerminal', 'closeTab', 'acknowledgeTerminal', 'detachTerminal', 'subscribeTerminal'];
  return methods.every(method => typeof api[method] === 'function') ? api as ManagerApi & TerminalApi : null;
}

export const transportError = (): AppError => ({
  code: 'INTERNAL',
  message: 'The manager did not confirm this request. No automatic retry was made. Refresh the session state before trying again; the operation may already have completed.',
  retryable: false,
});

export async function request<T>(operation: () => Promise<Result<T>>, timeoutMs = 30_000): Promise<Result<T>> {
  let timer: ReturnType<typeof setTimeout> | undefined;
  try {
    const pending = operation();
    if (timeoutMs === 0) return await pending;
    const timeout = new Promise<Result<T>>(resolve => {
      timer = setTimeout(() => resolve({ ok: false, error: transportError() }), timeoutMs);
    });
    // Timeout does not cancel the main operation. Its intent may have persisted.
    // Callers refresh state, but never automatically repeat a native mutation.
    return await Promise.race([pending, timeout]);
  } catch {
    return { ok: false, error: transportError() };
  } finally {
    if (timer !== undefined) clearTimeout(timer);
  }
}
