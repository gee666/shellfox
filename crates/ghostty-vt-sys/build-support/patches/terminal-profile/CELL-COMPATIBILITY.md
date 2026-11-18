# Legacy standalone zero-width cells

## Measured counterexamples

`tests/counterexample.c` was compiled with the verified project Zig 0.17.0 against the unmodified, signed official Windows vt1.0.1 DLL. `tools/counterexamples.mjs` compares its public plain formatter and cursor getters with the shipped xterm 6.0.0 default UnicodeV6 provider. The retained `tests/counterexamples.json` records the public line/copy text, cursor and xterm cell positions.

- `U+0301 A`: xterm copies hex `cc8141`, cursor `(2,0)`. Native Ghostty copies `41`, cursor `(1,0)`. This loses copy content and shifts A one column.
- `A SGR31 U+0301 B`: xterm cursor `(3,0)`, B at column 2. Native cursor `(2,0)`, B at column 1. Both copy `41cc8142`, but the displayed columns differ.
- `A VS16 B`: xterm copies `41efb88f42`; Native copies `4142`.

These are not evidence of complete terminal or raster parity. The xterm public buffer provides the displayed line/cell contents, not a captured GPU image.

## Why a width table alone is insufficient

The xterm parser resets its preceding-join property at dispatch/execute boundaries. A zero-width codepoint with preceding property zero is stored as text in its own slot, with cell width zero, while the cursor advances one slot. It is not a wide-character tail. Ghostty's current `Cell.Wide` has four values occupying two bits: narrow, wide, spacer tail, spacer head. None represents that text-bearing standalone case.

Treating it as an ordinary narrow cell would lie to render consumers. Treating it as a spacer would lose text/copy information. Attaching it to the preceding cell would change cursor and copy semantics. Reconstructing its parser state from surrounding cells would also be incorrect.

## Focused proposed representation

This is a design boundary, not an implemented success claim or permission to reinterpret upstream bytes.

1. Add an explicit standalone-zero-text discriminator to the internal fork cell representation. Preserve all existing Native cell field offsets and zero defaults. The prototype names one formerly reserved codepoint-payload bit as `shellfox_zero_text`, under explicit cell-layout version 2. It does not use the high native padding field, which raw-v1 temporarily uses for hyperlink IDs. Do not treat old reserved data as this flag.
2. Keep old C function signatures and Native values unchanged. Legacy consumers need an additive profile-aware semantic-width accessor that distinguishes a zero-text cell from a wide spacer, while retaining the physical grid slot. Existing `GhosttyCellWide` is insufficient to report this case.
3. Plain/copy formatting must retain the codepoint. Profile-aware VT formatting must establish the required zero preceding state before emitting a standalone cell; otherwise replay would join it to the prior cell. Maurice owns the ANSI formatter integration and must use the single shared UnicodeV6 module and State ABI.
4. Reflow and editing must move/copy/erase the explicit discriminator with the cell, preserving one physical slot but zero semantic glyph width. Tests must cover insertion, deletion, erase, wrap pending, both screens and resize, not only scalar printing.
5. Legacy snapshot bodies with this representation require a DISTINCT format version or magic and explicit bounded decode support. They must not use upstream `GHOSTSNP` raw version 1. The original raw-v1 decoder normalizes invalid codepoints to U+FFFD; it does NOT reject them. That behavior stays unchanged and is not repurposed to recover a zero-text flag. Old readers must reject NEW layout2 artifacts at the new envelope magic/version, before cell decoding. Every raw-v1 encoder must reject new cells. No new snapshot format is implemented by this focused prototype; Legacy decode/export stays unsupported. Brooklyn owns the matching codec/envelope work and atomic State/body cut.
6. State ABI1 remains the single sized struct containing terminal_profile and preceding_join. A cell-layout version belongs to the new snapshot format metadata, not a competing profile-state struct. Engine profile identity is not a substitute for that state.

Until the representation and matching codec/render paths are implemented and checked, Legacy live construction/decode/export remain unsupported checkpoints. The internal layout2 prototype now retains the measured leading/SGR/VS16 cell/copy/cursor cases; it also found that trim-right copy excludes a final zero-text slot, while the grid retains it. Its broad differential still fails the current cursor-line resize policy. Pending-wrap zero-text overflow remains separate unfinished work. These are valid xterm behaviors, not accepted final Legacy rejections, and no ASCII-only or complete Legacy parity is claimed.
