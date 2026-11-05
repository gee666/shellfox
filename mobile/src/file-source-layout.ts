/** Give an unwrapped native monospace input enough width, including tabs and wide glyphs.
 * Extra room is intentional: native TextInput has no cross-platform soft-wrap switch.
 */
export function sourceUnwrappedWidth(text: string): number {
  let columns = 0, longest = 0;
  for (const character of text) {
    if (character === '\n' || character === '\r') { longest = Math.max(longest, columns); columns = 0; }
    else if (character === '\t') columns += 8 - columns % 8;
    else columns += (character.codePointAt(0) ?? 0) >= 0x1100 ? 2 : 1;
  }
  return Math.ceil(Math.max(longest, columns) * 13 * 0.7 + 24);
}
