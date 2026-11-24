import { describe, expect, it } from 'vitest';
import { appliesWantedKeyboard, closesForIme, keyBarState, keyboardTap, specialTap, surfaceFocused, wantsKeyboardAfterTap,
  type KeyboardMode } from './key-bar-state';
import type { FocusResult } from './focus-feedback';

const modes: KeyboardMode[] = ['closed', 'system', 'special'];

describe('key bar state', () => {
  it('never shows both toggles active, and the keyboard toggle follows the IME only', () => {
    for (const mode of modes) for (const ime of [false, true]) {
      const state = keyBarState(mode, ime);
      expect(state.keyboardActive && state.specialActive).toBe(false);
      expect(state.keyboardActive).toBe(ime);
    }
    // A requested keyboard the OS hid (or never showed) is not shown as open.
    expect(keyBarState('system', false).keyboardActive).toBe(false);
  });

  it('shows the Fn toggle active only while its row is on screen', () => {
    expect(keyBarState('special', false)).toEqual({ keyboardActive: false, specialActive: true, specialRow: true });
    expect(keyBarState('special', true).specialActive).toBe(false);
    expect(keyBarState('closed', false).specialActive).toBe(false);
  });

  it('opens the keyboard from any hidden state and hides it only when it is shown', () => {
    expect(keyboardTap('closed', false)).toBe('show');
    expect(keyboardTap('special', false)).toBe('show');
    expect(keyboardTap('system', false)).toBe('reopen');
    for (const mode of modes) expect(keyboardTap(mode, true)).toBe('hide');
  });

  it('keeps every tap made while the IME is hidden as an open keyboard request', () => {
    for (const mode of modes) {
      expect(wantsKeyboardAfterTap(keyboardTap(mode, false))).toBe(true);
      expect(wantsKeyboardAfterTap(keyboardTap(mode, true))).toBe(false);
    }
  });

  it('asks for the keyboard again only when the terminal takes input and nothing holds the request', () => {
    // Tapped while reattaching: waits, then applies once input is ready.
    expect(appliesWantedKeyboard(true, 'closed', false, false)).toBe(false);
    expect(appliesWantedKeyboard(true, 'closed', false, true)).toBe(true);
    // Already requested from the view, shown, or never asked for.
    expect(appliesWantedKeyboard(true, 'system', false, true)).toBe(false);
    expect(appliesWantedKeyboard(true, 'closed', true, true)).toBe(false);
    expect(appliesWantedKeyboard(false, 'closed', false, true)).toBe(false);
    expect(appliesWantedKeyboard(true, 'special', false, true)).toBe(false);
  });

  it('toggles the Fn row by what is shown, closing it when the IME appears', () => {
    expect(specialTap('special', false)).toBe('closed');
    expect(specialTap('special', true)).toBe('special');
    expect(specialTap('system', true)).toBe('special');
    expect(closesForIme('special', false, true)).toBe(true);
    expect(closesForIme('special', true, true)).toBe(false); // still closing after a Fn tap
    expect(closesForIme('special', true, false)).toBe(false);
    expect(closesForIme('system', false, true)).toBe(false);
  });

  it('treats only an accepted terminal-surface focus as a keyboard request', () => {
    const result = (source: 'surface' | 'keyboard', focusRequest: 'accepted' | 'rejected'): FocusResult => ({
      platform: 'android', source, attached: true, windowFocused: true, uiThread: true, inputAllowed: true,
      frameAvailable: true, focusRequest, focused: focusRequest === 'accepted', keyboardRequest: 'accepted',
    });
    expect(surfaceFocused(result('surface', 'accepted'))).toBe(true);
    expect(surfaceFocused(result('surface', 'rejected'))).toBe(false);
    expect(surfaceFocused(result('keyboard', 'accepted'))).toBe(false);
  });
});
