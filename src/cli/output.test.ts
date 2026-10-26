import { expect, it } from 'vitest';
import { CliFailure, printError, printValue, unwrap } from './output';
import { CliUsageError } from './arguments';
function sink() { let text = ''; return { write(value: string) { text += value; }, value: () => text }; }
it('writes one JSON success record and preserves values and terminal escapes as data', () => {
  const output = sink();
  const value = { name: 'two words\n\u001b[31m\u009b31m\u202e', nested: [false, null, 2] };
  printValue(value, true, output);
  expect(JSON.parse(output.value())).toEqual({ ok: true, value });
  expect(output.value().split('\n')).toHaveLength(2);
  expect(output.value()).not.toContain('\u009b'); expect(output.value()).not.toContain('\u202e');
});
it.each([
  [new CliUsageError('bad option'), 2, 'VALIDATION', false],
  [new CliFailure('TARGET_LOST', 'uncertain write', true), 1, 'TARGET_LOST', true],
  [new Error('broken'), 1, 'INTERNAL', false],
])('writes JSON errors to stdout with no stderr or prompts', (error, status, code, retryable) => {
  const output = sink(), stderr = sink();
  expect(printError(error, true, output, stderr)).toBe(status);
  expect(JSON.parse(output.value())).toEqual({ ok: false, error: { code, message: error.message, retryable } });
  expect(stderr.value()).toBe('');
});
it('escapes human-facing errors and keeps them off stdout', () => {
  const output = sink(), stderr = sink();
  printError(new Error('\u001b[31munsafe\nline'), false, output, stderr);
  expect(output.value()).toBe(''); expect(stderr.value()).not.toContain('\u001b');
});
it('never treats a failed manager result as success', () => {
  expect(unwrap({ ok: true, value: 42 })).toBe(42);
  expect(() => unwrap({ ok: false, error: { code: 'NOT_FOUND', message: 'missing', retryable: false } })).toThrow(CliFailure);
});
