import type { AppError, Result } from '../../../src/shared/contracts';

export class TerminalOperationError extends Error {
  constructor(readonly detail: AppError) {
    super(detail.message);
    this.name = 'TerminalOperationError';
  }
}

export function unwrapTerminalResult<T>(result: Result<T>): T {
  if (!result.ok) throw new TerminalOperationError(result.error);
  return result.value;
}
