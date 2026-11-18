# KeyV2 proposal for review

Status: proposed, not implemented. No old `KeyEvent`, V1/Observer shape, C ABI
signature, default engine, GPUI/vendor code, or mobile capture changes belong to
this proposal. Russell owns manager validation and manager-v2 vocabulary. Eva
owns the pinned GPUI event/backend availability audit. Current M2 remains off.

## Evidence from the pinned public API

The signed manifest and hashes of the official source and Windows archives were
verified locally. This review read `key/event.h`, `key/encoder.h` and the actual
C implementation from source archive
`333dfce97641ef9b536bb9e08907ad86abaaada50e8f7417b70d9284adef37aa`.

The public event header SHA-256 is
`4a615f9864ae11bafa06cce7493aa6537804750ed4a6fda8b3f962ea348a8bc9`.
It matches the installed Native002 header. The source `key_event.zig` SHA-256 is
`596a66ee29253d2b705fe6247206e0d85bba5257aac83739eb280e4ffad39e0e`.
It also matches that unchanged Native002 file.

| Field | Current Rust/sys and C adapter | Actual pinned public field |
| --- | --- | --- |
| Physical key | `u32` / `uint32_t` | C enum, 176 values 0 through 175, excluding MAX sentinel |
| Action | `u32` / `uint32_t` | C enum, release 0, press 1, repeat 2 |
| Modifiers and consumed mask | `u32` / `uint32_t`, then unchecked casts | `GhosttyMods` is `uint16_t`; ten defined bits |
| Unshifted codepoint | `u32` / `uint32_t` | Public setter takes `uint32_t`, implementation truncates to `u21` |
| Composition | Rust `bool`, C argument `uint8_t` | Public setter takes C `bool`; adapter currently accepts any nonzero byte |
| Layout text | Borrowed Rust `&str`, pointer and `usize` | Borrowed pointer and `size_t`, not NUL-terminated |

`crates/ghostty-vt-sys/src/input.rs` currently bounds text size but does not
validate raw key/action/modifier/scalar fields. `input.c` casts the modifier
fields before calling the public setters. High bits can disappear there. The
pinned modifier setters copy their input without checking unknown bits. The
public UTF-8 setter borrows bytes without checking encoding or control content.
The scalar setter truncates instead of validating Unicode scalars. These checks
must happen in sys before any narrowing cast, not only in manager or the safe
wrapper.

The current safe wrapper has partial physical vocabulary and four modifier
booleans. Its `Key::Character` fallback returns zero for unsupported characters.
The approved immediate fix is manager-only rejection, not a layout change here.

## Exact additive API

The sys crate owns one typed vocabulary; the safe crate re-exports those new
value types. Do not duplicate 176 conversion tables between the two crates.
The complete proposed enum names and values are in
[the vocabulary table](ghostty-key-v2-vocabulary.md).

```rust
// sys, normal Rust types, not repr(C) event structures
#[repr(u32)]
pub enum PhysicalKey { /* exact 176 named variants, including Unidentified */ }
impl PhysicalKey {
    pub const ALL: [Self; 176];
    pub const fn code(self) -> u32;
}
impl TryFrom<u32> for PhysicalKey { type Error = InvalidKeyV2; }

#[repr(u32)]
pub enum KeyActionV2 { Release = 0, Press = 1, Repeat = 2 }
impl TryFrom<u32> for KeyActionV2 { type Error = InvalidKeyV2; }

pub struct ModifiersV2(u16); // private, only defined bits may be stored
impl ModifiersV2 {
    pub const NONE: Self;
    // Typed single-bit constants, BitOr and contains; no dependency needed.
    pub const SHIFT: Self;       // 0x001
    pub const CONTROL: Self;     // 0x002
    pub const ALT: Self;         // 0x004
    pub const SUPER: Self;       // 0x008
    pub const CAPS_LOCK: Self;   // 0x010
    pub const NUM_LOCK: Self;    // 0x020
    pub const SHIFT_SIDE: Self;  // 0x040
    pub const CONTROL_SIDE: Self; // 0x080
    pub const ALT_SIDE: Self;    // 0x100
    pub const SUPER_SIDE: Self;  // 0x200
    pub fn try_from_bits(bits: u32) -> Result<Self, InvalidKeyV2>;
    pub const fn bits(self) -> u16;
    pub const fn contains(self, flag: Self) -> bool;
}

pub struct KeyV2<'a> {
    pub physical: PhysicalKey,
    pub action: KeyActionV2,
    pub modifiers: ModifiersV2,
    pub consumed_modifiers: ModifiersV2,
    pub text: &'a str,
    pub unshifted_codepoint: Option<char>,
    pub composing: bool,
}
impl KeyV2<'_> {
    pub fn validate(&self) -> Result<(), InvalidKeyV2>;
}
impl Engine {
    pub fn encode_key_v2(&mut self, event: &KeyV2<'_>) -> sys::Result<Vec<u8>>;
}

// safe crate exports; all existing input exports remain unchanged
pub use sys::{PhysicalKey, KeyActionV2, ModifiersV2, InvalidKeyV2};
pub use sys::KeyV2 as KeyEventV2;
impl Terminal {
    pub fn encode_key_v2(&mut self, event: &KeyEventV2<'_>) -> Result<Vec<u8>>;
}
```

`PhysicalKey`, `KeyActionV2`, `ModifiersV2` are Copy, Eq and Debug. `KeyV2` is
Copy and Debug. It has no Default implementation. Callers provide all fields
explicitly. Enum conversion uses checked matches, never `transmute` or unchecked
numeric enum construction. One local table/macro may produce the enum, `ALL`
and checked numeric conversion without maintaining three lists.

`InvalidKeyV2` is a new ordinary Rust validation error with explicit variants
`PhysicalKey(u32)`, `Action(u32)`, `ModifierBits(u32)`, `Scalar(u32)`,
`TextCodepoint(char)` and `TextLimit(usize)`. It implements Display and Error.
The existing encoder result types stay unchanged. Sys maps invalid values to
-2 and text-budget errors to -6; callers may call `validate` for typed detail.
No validation error contains layout text or other user input beyond the offending
codepoint or byte count.

## Validation and preservation rules

- Accept precisely physical IDs 0 through 175 and actions 0 through 2. Zero is
  an explicit `Unidentified`, not a fallback for arbitrary input strings.
- Validate raw `u32` modifiers and consumed modifiers with `bits & !0x03ff == 0`
  before converting to `u16`. Preserve every combination of defined bits.
- Do not impose side/base coherence, side equality, or consumed-subset rules.
  The public header says side bits are only meaningful when the corresponding
  base modifier is active. It does not declare other bit patterns invalid.
  Release events must not fail because their base modifier has just gone up.
- Consumed modifiers are a mask, not another asserted physical keyboard state.
  The pinned implementation ignores that mask when text is empty; otherwise it
  computes effective modifiers as `mods & ~consumed_mods`. Preserve the original
  fields. Do not pre-subtract, infer consumed sides, or normalize lock bits.
- Raw scalar zero means absent. Otherwise require a Unicode scalar at most
  U+10FFFF and outside U+D800..U+DFFF before calling the public setter. For the
  typed event, `None` maps to zero. Reject `Some('\0')` because the public API
  cannot distinguish that from absent. Other valid unshifted scalars remain
  scalars, not physical-key guesses.
- Text is the unmodified, layout-produced UTF-8 sequence, including multi-scalar
  commits, combining sequences and non-ASCII text. Never Ctrl-transform it in
  manager or wrapper. `&str` proves UTF-8 validity. Reject C0 U+0000..U+001F,
  DEL U+007F and platform function-key PUA U+F700..U+F8FF as specified by the
  public text contract. Do not silently strip or replace them. C1 scalars are
  not excluded by that public guidance; do not reuse `char::is_control` and
  accidentally invent a broader ban in V2. The old wrapper remains unchanged.
- Retain the existing one-MiB input and output budgets. Reject text over budget
  before FFI; reject reported output over budget before allocation. Reject a
  changed or oversized second-call length, discard partial output, and retain
  existing reply-overflow poisoning behavior.
- `composing` is preserved, not used to manufacture committed input. The real
  encoder decides suppression and protocol output. IME preedit stays in the UI.
  Do not deliver the same commit once as a key and again as separate text.

A genuinely unavailable physical key with explicit layout/committed text is
legitimate. `Unidentified` plus text must remain usable for that case. Physical
identity must not be inferred from text, a platform function string or ASCII.

The public modifier representation cannot express independently held left and
right states or distinguish a known-left bit from an unavailable side reported
as the default zero bit. It has Caps/Num Lock state, but no Scroll/Fn Lock state
bits. ScrollLock and FnLock are physical identities only. V2 must not claim those
limitations disappeared or invent new public bits.

## Encoding and compatibility boundary

Reuse the existing `sf_key` signature. It already carries the necessary widths
and fields. The typed event converts to its existing arguments with widening
conversions after validation. Both old sys `encode_key(Key)` and new
`encode_key_v2` use one checked internal encode path. This prevents callers of
the existing public raw sys struct from bypassing numeric checks.

Keep the public Ghostty event allocation/setters/free and
`ghostty_key_encoder_set_from_terminal` plus `ghostty_key_encoder_encode`.
The pinned public encoder supports the size-query/copy pattern and does not
consume terminal input. Do not write a Rust CSI-u, Kitty, legacy or keypad
encoder. Known physical keys may legitimately encode no bytes in some modes;
representation of all 176 is not a promise of nonempty output for all 176.

Add defensive numeric checks in C `sf_key` before casts and event allocation.
Check physical/action bounds, both modifier masks, Unicode scalar, composing
byte 0/1 and pointer/length consistency. Rust remains responsible for validating
borrowed UTF-8 and its text policy before that internal FFI. No handwritten C
UTF-8 parser is needed. No old C argument order, width or event layout changes.

`set_from_terminal` resets macOS option-as-alt to false. This proposal retains
that existing policy. Right-side data survives transport; that does not make
side-specific Option-as-Alt configuration available in the wrapper. Exposing
that policy would require a separate reviewed additive configuration API, using
the existing public setter after `set_from_terminal`, not a framework patch or
a handmade encoder.

## Exact file scope after approval

| File | Intended change |
| --- | --- |
| `crates/ghostty-vt-sys/src/key_v2.rs` | New public event, action, error, validation and exports |
| `crates/ghostty-vt-sys/src/key_v2/physical.rs` | Single 176-entry closed enum table and checked conversions |
| `crates/ghostty-vt-sys/src/key_v2/modifiers.rs` | Private u16 mask, ten constants, checked u32 input |
| `crates/ghostty-vt-sys/src/key_v2/tests.rs` | Pure validation and exhaustive vocabulary tests |
| `crates/ghostty-vt-sys/src/input.rs` | Shared checked encoder call, additive V2 method |
| `crates/ghostty-vt-sys/src/lib.rs` | Add new module/exports, retain old `Key` fields |
| `crates/ghostty-vt-sys/input.c` | Before-cast defensive checks, unchanged function signature |
| `crates/ghostty-vt/src/input_v2.rs` | New exports and Terminal method only |
| `crates/ghostty-vt/src/lib.rs` | Register/re-export new module, retain old input exports |
| `crates/ghostty-vt/tests/input_v2.rs` | Real public encoder integration tests |
| `crates/ghostty-vt-sys/tests/input_v2_public.c` | Public-header enum/width assertions and setter/getter round trips |
| Crate READMEs | New usage, preservation limits and non-adoption scope |

The C test needs a narrow build-support test hook using the selected pinned
headers/library. Review that hook with the build owner before editing build.rs.
Keep each Rust module below 350 lines. No new dependencies or lockfile changes
are needed. A compiled adapter change requires fresh matching acceptance builds.

## Tests required before adoption

1. Compare all 176 names/discriminants with selected pinned public headers,
   including `KeyF` versus `F25`, international keys, keypad navigation/media
   keys and distinct Enter/NumpadEnter. Reject 176, INT_MAX and u32::MAX.
2. Assert public enum/int and modifier widths through C headers. Round-trip all
   176 keys and all 1024 defined modifier patterns through public event setters
   and getters, for active and consumed fields separately. Preserve right-only
   patterns on release as well as press/repeat. Reject every unknown modifier
   bit, especially bit 10, bit 15, bit 16 and bit 31, before narrowing.
3. Reject raw scalars in the surrogate range, U+110000, u32::MAX and values that
   would become valid after u21 truncation. Exercise absent, ASCII, supplementary
   scalars and the typed NUL ambiguity.
4. Exercise valid UTF-8 multi-scalar text and C1, forbidden C0/DEL/function PUA,
   exact budget boundaries, empty text, IME composition and explicit unknown
   physical identity with committed text. No lossy decoding or key guessing.
5. Compare V2 output with direct public Ghostty event/encoder calls for the
   same state. Cover ordinary text, Ctrl-C, shifted/consumed text, keypad modes,
   Num/Caps Lock, application cursor, modifyOtherKeys, Kitty alternate/associated
   text, press/repeat/release and composing. Check no-output events explicitly.
6. Keep old wrapper/sys input and paste regressions. Confirm all old types and
   the `sf_key` C declaration stay unchanged. The approved manager Character
   tests belong to Russell; they must reject unsupported ASCII without turning
   legitimate explicit `Unidentified` IME commits into failures.
7. Run formatting, Clippy, workspace/root/mobile typecheck and relevant unit and
   native integration tests. Run actual supported-target acceptance after the
   compiled adapter change. A host pass is not Windows/macOS/X11/Wayland/mobile
   capture acceptance. GPUI availability remains Eva's separate review.

No implementation or platform/default adoption is authorized by this document.
