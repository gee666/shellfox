import type { AppError } from '../../../src/shared/contracts';

export class TerminalOperationError extends Error {
  constructor(readonly detail: AppError) {
    super(detail.message);
    this.name = 'TerminalOperationError';
  }
}
