# Input vocabulary and encoded-budget audit

This audit records the pre-V2 API/frontend gaps. Maurice's full safe producer and Runtime's [owned KeyV2 branch](key-v2-producer.md) are now compiled, with 176 checked physical values and full modifier bits. Existing legacy Key and OS/frontend capture remain separate; no current manager/native OS mapping is declared complete. No manual escape encoder or Observer/V1 schema change is proposed.

## Current producer vocabulary

Sources inspected: `crates/ghostty-vt/src/input.rs`, `crates/ghostty-vt-sys/src/input.rs`, the pinned libghostty-vt 1.0.1 `include/ghostty/vt/key/event.h`, `crates/shellfox-desktop/src/input.rs`, and the manager-v2 key/input DTOs.

| Input identity/state | Current Rust wrapper and lossless manager mapping | Gap relative to pinned public Ghostty ABI |
| --- | --- | --- |
| Enter, Tab, Backspace, Escape, arrows, Home/End, PageUp/Down, Insert/Delete | Same named variant to same `Key` variant. These are 14 named variants, plus Unidentified separately. | Numpad Enter must NOT map to ordinary Enter. |
| F1..F25 | Function(number) to `Key::F(number)` with closed 1..25 validation. | Other function/system/media identities are absent. |
| Physical ASCII positions | Letters a-z, digits 0-9, backquote, backslash, brackets, comma, equal, minus, period, quote, semicolon, slash and space map to matching `Key::Character`. Uppercase ASCII letters normalize to their same physical letter position. Text stays separate. | The current wire Character validator accepts MORE ASCII symbols than `Key::code` supports. Shifted symbols such as !, + or ? currently become native key code 0. Do not call that lossless or silently perform this conversion. |
| UTF-8 committed logical text | Preserve `text` independently from physical key and preserve unshifted scalar. Unicode text itself is supported subject to native validation. | Non-ASCII `Character` is NOT a physical-key mapping: current `Key::code` maps it to 0. Text does not recover missing physical identity or key location. |
| Press/repeat/release | Exact `KeyAction` mapping exists. | Release must use the same actual physical key identity. Unsupported key identity cannot be reconstructed from its committed text on release. Actual bytes depend on the owner's terminal modes; zero encoding is not a lost-event fallback. |
| Composition | The wrapper carries `composing`, nullable committed UTF-8 text and `unshifted_codepoint`. Preserve them exactly. | Desktop GPUI adapter currently supplies committed text as Unidentified/Press and sets composing=false. This is not proof of a complete physical/logical/IME lifecycle adapter. |
| Shift/Control/Alt/Super and consumed modifiers | Exact four booleans map to the same four existing native fields. | Lock state and modifier side information cannot fit these fields. |
| Keypad digits/add/operators/Enter/navigation | NONE in current Rust Key enum or manager DTO. | ABI has distinct NUMPAD_0..9 (80..89), ADD (90), ENTER (97), many other operations/navigation through 119. Ordinary digits/+ or Enter are NOT equivalent in application keypad/extended keyboard modes. |
| CapsLock/NumLock | NONE as identity or state in current wrapper/DTO. | ABI has CAPS_LOCK key 54 and NUM_LOCK key 79; modifier-state bits 4 and 5 are distinct from the key events. |
| International physical keys/IME controls | NONE in current wrapper/DTO. | ABI has INTL_BACKSLASH/RO/YEN (17..19), CONVERT/KANA_MODE/NON_CONVERT (65..67). Do not collapse them to US punctuation or Unidentified. |
| Left/right modifier identities/state | NONE in current wrapper/DTO. | ABI has separate modifier key identities and side bits 6..9. Side state is meaningful only with its corresponding pressed bit. |

The actual pinned ABI is W3C physical-code based and broader than the existing Rust wrapper. The four-modifier limitation is therefore a current NATIVE gap too, not a transport-only omission. The manager DTO's current representable subset can map exactly only after rejecting unsupported Character identities and text values that the wrapper itself rejects. Native text validation rejects control characters and U+F700..U+F8FF and validates unshifted Unicode scalars.

Current desktop behavior is also incomplete: `physical_key` falls back to Unidentified for unknown multi-character key names, and arbitrary one-scalar logical names become Character even when no supported physical position exists. Unmodified printable input deliberately uses the committed-text path. No audit result here upgrades that to complete keypad/international/lock/IME support.

## Mapping rule before native mapping freezes

1. Preserve physical identity, logical committed text, location/keypad distinction, action, consumed modifiers, modifier state and composition separately when those facts exist.
2. Map only the exact supported native variants above. An unsupported KNOWN identity is unsupported, not genuine Unidentified. Reject/report it before enqueue instead of silently dropping identity.
3. Genuine Unidentified may represent an actually unidentified committed-text event; it is not a bucket for recognized keypad/international keys.
4. Complete support needs a coordinated typed Rust wrapper + frontend + manager vocabulary extension against the pinned public ABI, with working owner encoding and mode-enabled tests. It does not need a handwritten escape encoder or an unbounded arbitrary native integer code.
5. Until that work is reviewed and shipped, report the gaps. Do not claim complete keyboard input from the present DTOs. Do not change existing Observer/V1 schemas to disguise the gap.

## Payload versus encoded receipt budget

Current 64 KiB payload limit is distinct from actual encoded byte count. Bracketed paste adds 12 bytes. Kitty associated-text encoding can expand a 64 KiB committed-text payload by much more than 12 bytes. A successful receipt must report the COMPLETE actual owner-encoded write, not min(payload length, 65536), a client-supplied count or a truncated buffer.

The existing Ghostty sys encoder already bounds its temporary encoded allocation at 1 MiB. Runtime's existing legacy accept_input and terminal write request each limit 64 KiB. Therefore merely widening manager AcceptedInput.validate would not make a 65536-byte bracketed paste succeed.

The proposed 1 MiB multi-write expansion is explicitly ON HOLD and was NOT implemented. The source now names the two distinct limits even though both remain 64 KiB:

- `MAX_INPUT_PAYLOAD_BYTES = 65536`: admission payload bound. `MAX_INPUT_BYTES` remains its compatibility alias.
- `MAX_ENCODED_INPUT_BYTES = 65536`: actual encoded write/receipt ceiling, checked after owner encoding and BEFORE prewrite or any write.
- One existing bounded native write only. No splitting, truncation, prefix insertion, synthesized count or automatic retry.
- A valid 65536-byte text payload may fail the encoded resource limit. For example bracketed paste needs 65548 encoded bytes. This is an explicit parity gap, not complete input acceptance.
- Stamped controller epochs report a typed ResourceLimit for encoded overflow; ordinary legacy epochs keep their existing validation behavior. Neither path emits a successful receipt or invokes the prewrite hook on overflow. Observer/V1 wire behavior remains unchanged.

Consumers use the encoded ceiling for PREPARED native-byte and returned native-receipt checks. Raw binary admission uses the payload ceiling and successful raw receipts must equal the original decoded byte length. The current manager receipt ceiling stays 65536. That validation ceiling is correct for this reviewed source budget; it must not be widened independently or populated with a clipped count.

Actual bracketed-paste boundary gates are 65524 payload -> 65536 encoded accepted, and 65525/65536 valid payload -> deterministic prewrite ResourceLimit with no hook/write/receipt. Preserve and compare the original RequestId and actual prepared/accepted count. Mode-enabled key tests must likewise reject expansion before any side effect when it exceeds the encoded cap. Tests of DTO-only facts do not replace actual owner encoding evidence.

Before any expansion, publish and review bounded asynchronous sequencing, confirmed-prefix versus uncertain-write receipts, once-only activity/settlement accounting, control-loss fencing and a global queued-byte budget. Do not add a blocking multi-write owner loop or treat an acknowledged prefix as complete success.
