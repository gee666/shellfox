import { describe, expect, it } from 'vitest';
import { emptyModifiers } from '../terminal/protocol';
import { keyInput, keyRelease } from './key-input';

const event = (input: ReturnType<typeof keyInput>) => {
  if (input.type !== 'key') throw new Error('not a key');
  return input.event;
};

describe('key bar keys', () => {
  it('names Ghostty keys, layout text and modifiers', () => {
    const ctrl = { ...emptyModifiers, ctrl: true };
    expect(event(keyInput('Escape', emptyModifiers))).toMatchObject({ physical: 120, text: '', modifiers: 0 });
    expect(event(keyInput('Tab', emptyModifiers))).toMatchObject({ physical: 64 });
    expect(event(keyInput('ArrowLeft', ctrl))).toMatchObject({ physical: 76, modifiers: 2 });
    expect(event(keyInput('F12', emptyModifiers))).toMatchObject({ physical: 132 });
    expect(event(keyInput('c', ctrl))).toMatchObject({ physical: 22, text: 'c', unshiftedCodepoint: 99, modifiers: 2 });
    expect(event(keyInput('a', { ...emptyModifiers, shift: true }))).toMatchObject({ text: 'A', consumedModifiers: 1, modifiers: 1 });
    expect(event(keyInput('a', { ...emptyModifiers, capsLock: true }))).toMatchObject({ text: 'A', modifiers: 16 });
    expect(event(keyInput('|', emptyModifiers))).toMatchObject({ physical: 2, text: '|', unshiftedCodepoint: 92, modifiers: 1, consumedModifiers: 1 });
    expect(event(keyInput('|', ctrl))).toMatchObject({ physical: 2, modifiers: 3, consumedModifiers: 1 });
    expect(event(keyInput('2', { ...emptyModifiers, shift: true }))).toMatchObject({ physical: 8, text: '@' });
    expect(event(keyInput('/', emptyModifiers))).toMatchObject({ physical: 50, text: '/' });
    expect(event(keyInput(' ', ctrl))).toMatchObject({ physical: 63, text: ' ', unshiftedCodepoint: 32 });
  });
  it('releases the same key without text', () => {
    const press = keyInput('c', { ...emptyModifiers, ctrl: true });
    expect(event(keyRelease(press))).toEqual({ ...event(press), action: 'release', text: '' });
  });
});
