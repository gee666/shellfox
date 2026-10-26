import { CliUsageError } from './arguments';
import type { Result } from '../shared/contracts';
import { colorEnabled, renderHuman } from './human';
import { escapeTerminal, styles } from '../main/ssh/presentation';
export interface TextOutput { write(text: string): unknown; isTTY?: boolean }
export class CliFailure extends Error {
  constructor(readonly code: string, message: string, readonly retryable = false) { super(message); }
}
export function unwrap<T>(result: Result<T>): T {
  if (!result.ok) throw new CliFailure(result.error.code, result.error.message, result.error.retryable);
  return result.value;
}
function encode(value: unknown): string {
  return JSON.stringify(value).replace(/[\u007f-\u009f\u2028\u2029\u202a-\u202e\u2066-\u2069]/g,
    char => '\\u' + char.charCodeAt(0).toString(16).padStart(4, '0'));
}
export function printValue(value: unknown, json: boolean, output: TextOutput): void {
  if (json) output.write(encode({ ok: true, value: value ?? null }) + '\n');
  else output.write(renderHuman(value, colorEnabled(output)) + '\n');
}
export function printError(error: unknown, json: boolean, output: TextOutput, stderr: TextOutput): number {
  const code = error instanceof CliUsageError ? 'VALIDATION' : error instanceof CliFailure ? error.code : 'INTERNAL';
  const message = error instanceof Error ? error.message : 'Command failed.';
  const retryable = error instanceof CliFailure && error.retryable;
  if (json) output.write(encode({ ok: false, error: { code, message, retryable } }) + '\n');
  else stderr.write(styles(colorEnabled(stderr)).red('Shellfox: ') + escapeTerminal(message) + '\n');
  return error instanceof CliUsageError ? 2 : 1;
}
