import { describe, expect, it } from 'vitest';
import { nativeViewFailure } from './native-view-failure';

describe('native view failures', () => {
  it('drops refused input, reports a large paste and reopens only for engine failures', () => {
    expect([1, 3, 7].map(nativeViewFailure)).toEqual(['dropped', 'dropped', 'dropped']);
    expect(nativeViewFailure(2)).toBe('paste');
    expect([4, 5, 6, 8, 11].map(nativeViewFailure)).toEqual(Array(5).fill('engine'));
  });
});
