import type { FocusResult } from './focus-feedback';

/** What the user asked the key bar for; the IME reports what is actually shown. */
export type KeyboardMode = 'closed' | 'system' | 'special';

/**
 * As in 0.9.15, the two toggles are never active together: the keyboard
 * toggle follows real IME visibility, and the Fn row only shows while the IME
 * is hidden.
 */
export function keyBarState(mode: KeyboardMode, imeVisible: boolean) {
  const specialRow = mode === 'special' && !imeVisible;
  return { keyboardActive: imeVisible, specialActive: specialRow, specialRow };
}

/**
 * Keyboard toggle. `reopen` means the terminal already wants the IME but it
 * is not shown (hidden by the OS, or never shown), so focus must be requested again.
 */
export function keyboardTap(mode: KeyboardMode, imeVisible: boolean): 'hide' | 'show' | 'reopen' {
  if (imeVisible) return 'hide';
  return mode === 'system' ? 'reopen' : 'show';
}

/**
 * A keyboard request waits until the terminal can take input. A tap made
 * while it is attaching or recovering, or one dropped by a reattach before
 * the IME appeared, is not lost. It ends when the IME appears or the user
 * chooses something else.
 */
export function wantsKeyboardAfterTap(tap: 'hide' | 'show' | 'reopen'): boolean {
  return tap !== 'hide';
}

/** Ask for the system keyboard again: the request is still open and nothing holds it. */
export function appliesWantedKeyboard(wanted: boolean, mode: KeyboardMode, imeVisible: boolean, inputEnabled: boolean): boolean {
  return wanted && inputEnabled && !imeVisible && mode === 'closed';
}

export function specialTap(mode: KeyboardMode, imeVisible: boolean): 'closed' | 'special' {
  return keyBarState(mode, imeVisible).specialRow ? 'closed' : 'special';
}

/** The native view took focus from a tap on the terminal itself. */
export function surfaceFocused(result: FocusResult): boolean {
  return result.source === 'surface' && result.focusRequest === 'accepted';
}

/** The IME appearing closes the Fn row, as in 0.9.15. Only the appearance
 * counts: an IME that is still closing after a Fn tap must not undo it. */
export function closesForIme(mode: KeyboardMode, wasVisible: boolean, imeVisible: boolean): boolean {
  return imeVisible && !wasVisible && mode === 'special';
}
