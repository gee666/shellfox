/** UI Automator keeps clipped controls in its tree with zero-area bounds. */
export function controlBounds(node) {
  if (typeof node?.bounds !== 'string') return undefined;
  const match = /^\[(\d+),(\d+)\]\[(\d+),(\d+)\]$/.exec(node.bounds);
  if (!match) return undefined;
  const [, left, top, right, bottom] = match.map(Number);
  if (right <= left || bottom <= top) return undefined;
  return { left, top, right, bottom };
}

export function visibleControl(tree, text) {
  return tree.find(node => node.label === text && controlBounds(node))
    ?? tree.find(node => node.text === text && controlBounds(node));
}

/** Back dismisses a prompt when a hardware keyboard keeps the software IME hidden. */
export function keyboardIsShown(dump) {
  const state = /^\s*mInputShown=(true|false)\b/m.exec(dump)
    ?? /^\s*mIsInputViewShown=(true|false)\b/m.exec(dump);
  return state?.[1] === 'true';
}

export function needsKeyboardDismissal(node, inputMethodDump) {
  // Floating IMEs can be visible without covering the action. Back can then
  // close the modal, so prefer tapping an already drawable action directly.
  return !controlBounds(node) && keyboardIsShown(inputMethodDump);
}

/** Search below and above the current scroll position, with a bounded traversal. */
export async function revealControl(text, { read, scroll, attempts = 6 }) {
  let found = visibleControl(await read(), text);
  if (found) return found;
  for (const direction of ['forward', 'backward']) {
    for (let i = 0; i < attempts; i++) {
      await scroll(direction);
      found = visibleControl(await read(), text);
      if (found) return found;
    }
  }
  throw new Error(`Could not reach ${text}.`);
}
