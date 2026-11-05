import { expect, it } from 'vitest';
import { sourceUnwrappedWidth } from './file-source-layout';

it('sizes unwrapped native source by longest line, preserving room for tabs and wide glyphs', () => {
  expect(sourceUnwrappedWidth('')).toBe(24);
  expect(sourceUnwrappedWidth('short\nlonger')).toBe(sourceUnwrappedWidth('longer'));
  expect(sourceUnwrappedWidth('\t')).toBe(sourceUnwrappedWidth(' '.repeat(8)));
  expect(sourceUnwrappedWidth('a\t')).toBe(sourceUnwrappedWidth(' '.repeat(8)));
  expect(sourceUnwrappedWidth('日本')).toBe(sourceUnwrappedWidth('xxxx'));
  expect(Number.isFinite(sourceUnwrappedWidth('x'.repeat(100_000)))).toBe(true);
});
