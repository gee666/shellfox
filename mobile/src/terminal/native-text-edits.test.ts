import { describe, expect, it } from 'vitest';
import { terminalTextEdit } from './native-text-input';

// A line editor oracle checks the resulting text AND caret, not just escape bytes.
function apply(value: string, cursor: number, input: string) {
  const line = Array.from(value);
  for (let i = 0; i < input.length;) {
    if (input[i] === '\x1b') {
      const direction = input[i + 2];
      if (!['D', 'C'].includes(direction ?? '')) throw new Error('Unexpected cursor sequence');
      cursor += direction === 'D' ? -1 : 1;
      i += 3;
    } else if (input[i] === '\x7f') {
      if (cursor > 0) line.splice(--cursor, 1);
      i++;
    } else {
      const character = String.fromCodePoint(input.codePointAt(i)!);
      line.splice(cursor++, 0, character);
      i += character.length;
    }
    if (cursor < 0 || cursor > line.length) throw new Error('Cursor left the editable line');
  }
  return { value: line.join(''), cursor };
}
const state = (value: string, cursor: number) => ({ value, cursor });

describe('mobile native edit line-editor round trips', () => {
  for (const applicationCursor of [false, true]) {
    it(`applies every range replacement and caret position, application cursor=${applicationCursor}`, () => {
      // Includes repeated characters, punctuation, RTL, CJK and surrogate pairs.
      for (const value of ['', 'a', 'aaaa', 'banana', 'a b!?', '日本語', 'אבגד', 'a🙂b']) {
        const points = Array.from(value);
        const offset = (index: number) => points.slice(0, index).join('').length;
        for (let cursor = 0; cursor <= points.length; cursor++) {
          for (let start = 0; start <= points.length; start++) {
            for (let end = start; end <= points.length; end++) {
              for (const replacement of ['', 'a', 'aa', ' !', '🙂', '日本']) {
                const next = points.slice(0, start).join('') + replacement + points.slice(end).join('');
                const nextPoints = Array.from(next);
                for (const nextCursor of new Set([0, start + Array.from(replacement).length, nextPoints.length])) {
                  const before = state(value, offset(cursor));
                  const after = state(next, nextPoints.slice(0, nextCursor).join('').length);
                  const input = terminalTextEdit(before, after, applicationCursor);
                  expect(apply(value, cursor, input), JSON.stringify({ before, after, applicationCursor }))
                    .toEqual(state(next, nextCursor));
                }
              }
            }
          }
        }
      }
    });
  }
  it('can delete every character after repeated edits in the middle of a word', () => {
    let current = state('banana', 6);
    for (const next of [state('baxnana', 3), state('banna', 2), state('anna', 0), state('nna', 0), state('na', 0), state('a', 0), state('', 0)]) {
      current = apply(current.value, current.cursor, terminalTextEdit(current, next));
      expect(current).toEqual(next);
    }
  });
});
