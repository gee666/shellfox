# Pinned GPUI keyboard audit for KeyV2 review

Status: source audit only. No GPUI/vendor/backend/subclass edits, GUI input trials or KeyV2 implementation were made. Current manager-v2 input remains incomplete/off.

The dependency is `gpui = 0.2.2`, pinned in `crates/shellfox-desktop/Cargo.toml` and `Cargo.lock`. GPUI paths below are relative to that crate's `src/` in the Cargo registry, not this repository.

## Conclusion

Existing GPUI APIs can preserve logical key strings, UTF-8 committed text, aggregate modifiers, Caps Lock state, key down/up, and repeat on some backends. They cannot supply a layout-independent full physical vocabulary, left/right modifier state, Num Lock, consumed-modifier masks, or a native composition flag tied to each physical event.

Do not derive physical identity from `Keystroke.key`, treat missing information as proven false, or map arbitrary ASCII/unknown names to physical code zero. A wrapper that can encode 176 pinned physical keys does not make all those keys observable through this GPUI version.

## Public API evidence

| API | Actual public fields | Consequence |
| --- | --- | --- |
| `Keystroke`, `platform/keystroke.rs:18` | `modifiers`, `key: String`, `key_char: Option<String>` | No native code, scan code, key location or physical identity. `key` is explicitly a printed-label/ASCII-equivalent binding representation. |
| `Modifiers`, same file:447 | Five bools: control, alt, shift, platform, function | No sides, Num Lock, Scroll Lock or consumed modifiers. |
| `KeyDownEvent`, `interactive.rs:22` | `keystroke`, `is_held` | Down/repeat can be distinguished only when the backend supplies that distinction. |
| `KeyUpEvent`, same file:40 | `keystroke` | No physical release identity or native location. |
| `ModifiersChangedEvent`, same file:55 | `modifiers`, `capslock` | Aggregate state changes, not distinct physical modifier press/release events. |
| `Capslock`, `platform/keystroke.rs:656` | `on: bool` | Caps Lock only. |
| `Window::modifiers/capslock`, `window.rs:1898` | Last framework-tracked state | Useful public state, not an atomic full native snapshot attached to each key event. |
| `EntityInputHandler`, `input.rs` | Commit, marked/preedit, unmark and UTF-16 text ranges | Can preserve committed UTF-8 and UI composition state. No association with the physical key that produced a commit. |

`PlatformWindow` is crate-private, `platform.rs:460`. Its public window handle is not a public raw-key event stream. Polling native keyboard state through a handle would not recover an event-atomic identity and is not proposed as a workaround.

## Backend findings

### Windows

Evidence: `platform/windows/events.rs:329-420, 1299-1428, 1471-1493`; `platform/windows/keyboard.rs:182-214`.

- The native handler receives WPARAM virtual key and LPARAM. The current parser extracts the eight-bit scan code for character conversion; the public event does not retain it. The extended-key bit is not exposed to consumers.
- Shift, Control, Alt, Windows and Caps Lock are routed to `ModifiersChanged`, not ordinary physical key press/release events. Left/right identity is lost.
- `current_modifiers` samples left/right Alt and left Control for AltGr, then deliberately removes synthetic Control/Alt when the layout uses AltGr. Public consumers cannot reconstruct raw side state or a consumed-modifier mask from that result.
- Shift is cleared for shifted symbols in `get_keystroke_key`. The key-event modifier set is a binding normalization, not the full physical state.
- Immutable parsing maps Return to `enter` without keypad location. It lists F1-F24. Unknown non-character virtual keys can fail character conversion and produce no public key event. No WM_APPCOMMAND route was found in this event dispatcher.
- Repeat is preserved through LPARAM bit 30 to `is_held`.
- Caps Lock has a public state getter/change event. Num Lock and Scroll Lock are not exposed. Function is hardcoded false; ordinary Win32 messages do not guarantee visibility of hardware Fn.
- WM_CHAR decodes UTF-16 surrogate pairs and delivers text via the input handler. During marked-text composition, normal keydown is sent to TranslateMessage rather than the normal public key callback. There is no per-key composing field.

Native scan/VK/extended information and per-side state would need a reviewed framework event addition before Shellfox could preserve them.

### macOS

Evidence: `platform/mac/events.rs:84-137, 297-454`; `platform/mac/window.rs:1683-1759`.

- NSEvent keyCode is available to the backend and used for layout translation, but is discarded before constructing `Keystroke`.
- Binding translation intentionally changes layouts for Command shortcuts, handles non-ASCII layouts and clears Shift for some symbols. The resulting `key` is not a hardware key position.
- Aggregate Control/Option/Shift/Command/Function and Caps Lock are available. NSFlagsChanged exposes only aggregate public state, not the native modifier-key identity. The backend does not expose device-side modifier bits.
- Return and keypad Enter are collapsed to `enter`; special logical names include F1-F35.
- NSEvent repeat reaches `is_held`.
- The input context gets first refusal for composition and some nonprinting keys. A key handled there need not reach the normal key callback. Commits/preedit remain available through `EntityInputHandler`, without physical correlation.

A framework change must retain native keyCode and a verified modifier snapshot before layout/IME normalization. The exact public native source for complete side/lock state still needs platform review; do not substitute the numeric-pad event flag for a Num Lock toggle.

### X11

Evidence: `platform/linux/x11/client.rs:553-574, 949-1074`; `platform/linux/platform.rs:805-1018`; `platform/linux/x11/event.rs:35-56`.

- Core KeyPress/KeyRelease `detail` supplies an XKB keycode. It is passed to `Keystroke::from_xkb`, then discarded.
- Modifier keysyms are filtered from down/up callbacks. XKB state notifications retain depressed/latched/locked masks internally, but only aggregate modifiers and Caps Lock are published.
- `from_xkb` resolves keysyms/UTF-8, falls back to a guessed ASCII position, clears Shift for symbols and strips `kp_` in its keypad fallback. Layout-independent identity and several keypad distinctions cannot be recovered afterward.
- The event batcher removes a nearby same-key release as presumed autorepeat. The subsequent public keydown still sets `is_held: false`. Existing public APIs do not reliably distinguish press from repeat on this backend.
- XKB composition rewrites `key`/`key_char`; preedit/commit callbacks exist separately.
- XKB device state is initially loaded, but the client initializes its exported modifier/Caps Lock cache to defaults and updates it on notifications. Do not claim a complete initial/full-state snapshot from those bools.

A reviewed framework event path must retain the keycode, keymap identity/location mapping and raw modifier snapshot before keysym conversion/filtering. It must also make repeat explicit rather than relying on Shellfox timing guesses.

### Wayland

Evidence: `platform/linux/wayland/client.rs:1258-1405, 1432-1485`; common XKB conversion above.

- wl_keyboard supplies an evdev key. GPUI adds the protocol/XKB offset of eight, converts it, and discards the numeric identity.
- wl_keyboard modifier events provide depressed/latched/locked masks and group. Public output again has aggregate modifiers and Caps Lock only.
- Modifier keysyms are filtered from physical down/up delivery. The common XKB conversion has the same keypad/Shift/ASCII-normalization losses as X11.
- GPUI's repeat timer emits `is_held: true`, so repeat is available here. The repeat callback captures the converted keystroke from the initial press rather than rebuilding a fresh full modifier snapshot each tick.
- Text-input-v3 commit strings of one byte synthesize a public keydown with default modifiers. Longer commits use the text-input handler. Such a synthetic keydown is not evidence of a physical key or physical repeat/release lifecycle.

The extension must preserve actual evdev identity and distinguish physical input from synthesized text commits. It must obtain lock/side state from validated keyboard state, not assume fixed modifier bit numbers across keymaps.

## Existing Shellfox adapter limitations

- `crates/shellfox-desktop/src/input.rs::physical_key` currently consumes the logical GPUI string, not a physical identifier. Unknown strings become legacy `Unidentified`; only F1-F25 are recognized. This function must not become the KeyV2 physical mapper.
- `encode_physical` currently forwards four aggregate modifiers and fills consumed modifiers with defaults. It cannot honestly populate new lock/side state from those fields.
- `src/app/selection.rs` converts `is_held` into Repeat. That preserves what Windows/macOS/Wayland supply but cannot repair X11's false repeat flag.
- `src/app/runtime/input.rs` releases against the same logical-derived key. A future physical held-key table must use the actual physical identity, including keypad/location, without changing enqueue-time settlement capture.
- `src/composition.rs` already keeps preedit separate from committed text. It preserves the commit string exactly once, but receives no originating physical code. A text-only event must stay explicitly text-only.

## Review proposal, not implementation authorization

1. Maurice's additive wrapper/sys KeyV2 remains the encoder boundary. Shellfox must call the pinned public Ghostty encoder, not construct terminal escape sequences.
2. Keep manager-v2 physical vocabulary closed and validated against that pinned key set. Unknown/arbitrary ASCII identifiers must reject explicitly or remain an explicit unknown/text-only state, never silently become key zero.
3. Review an additive GPUI raw/native key observation path, separate from existing normalized keybinding semantics. It needs native physical identity, event action/repeat, raw and consumed modifier state, state availability, lock/side information, composition/provenance and focus-loss reconciliation. Do not overload `Keystroke.key`.
4. A minimally reviewable native identifier would preserve platform code provenance, for example Windows VK plus scan/extended information, macOS keyCode, or XKB/evdev keycode with origin/keymap context. The exact type/mapping must be reviewed before code; these numbers are not already Ghostty enum values.
5. Capture raw metadata before modifier-key filtering, layout normalization and IME consumption. Keep committed text separate so one physical event plus a native commit cannot type twice. Add an explicit provenance distinction for Wayland's synthetic one-byte commits.
6. Framework files implicated: `interactive.rs`, `platform/keystroke.rs`, `interactive`/`window` dispatch, Windows `events.rs`, macOS `events.rs` and `window.rs`, Linux common `platform.rs`, X11 `client.rs`/`event.rs`, Wayland `client.rs`. No edits are approved by this audit. An upstream-compatible version/API should be evaluated before choosing any fork.
7. After framework/API review, Shellfox integration points are `input.rs`, `composition.rs`, `app/selection.rs`, `app/runtime/input.rs` and the typed runtime/native converter. Keep existing lifecycle, controller/observer and frozen input-context fences unchanged.

## Proposed tests before claiming desktop coverage

- Layout-independent A/Q and Y/Z under QWERTY/AZERTY/QWERTZ/Dvorak; non-ASCII layout and dead-key/AltGr cases. Assert physical identity separately from logical/committed text.
- Main Enter versus keypad Enter; keypad digit/navigation with Num Lock on/off; ISO extra key, F24/F25/F35 where observable, and unsupported media keys. No string fallback to code zero.
- Left/right Shift, Control, Alt/Option and Super/Command, both sides held, lock state across focus changes. Unsupported/unobserved state is not tested as known false.
- Press/repeat/release on every backend, including X11 detectable/core repeat and modifier-only transitions; loss of focus and shutdown synthesize only the already-approved cancellation releases.
- IME/preedit/commit with multi-scalar/non-BMP UTF-8, Wayland one-byte synthetic commit, empty commits, no duplicate text or forged physical identity.
- Backend-native metadata fixture tests plus owned OS input trials on each platform after exclusive test-window approval. A passing safe-wrapper encoder test does not establish GPUI capture coverage.

This audit made no runtime acceptance claim. Live GUI testing remains paused; M2 must not be enabled on the strength of public GPUI key strings alone.
