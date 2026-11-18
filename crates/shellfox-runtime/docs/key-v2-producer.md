# Owned KeyV2 Runtime producer

This consumes Maurice's compiled safe Ghostty KeyEventV2 API. It adds no wire schema, native OS capture, permission or hand-written escape encoder. Existing Command::Key stays unchanged.

## Exact compiled API

```rust
Command::KeyV2 { lease: Lease, event: OwnedKeyEventV2 }
    -> Response::Input(InputReceipt)

pub struct OwnedKeyEventV2 {
    pub physical: PhysicalKey,
    pub action: KeyActionV2,
    pub modifiers: ModifiersV2,
    pub consumed_modifiers: ModifiersV2,
    pub text: String,
    pub unshifted_codepoint: Option<char>,
    pub composing: bool,
}
```

Runtime reexports OwnedKeyEventV2 plus the SAME safe producer PhysicalKey, KeyActionV2, ModifiersV2 and InvalidKeyV2 value types. There is no second physical enum/discriminant table. Use PhysicalKey::try_from(code), KeyActionV2::try_from(value) and ModifiersV2::try_from_bits(bits) for checked boundary conversion. All 176 PhysicalKey values come from Maurice's public-ABI table.

OwnedKeyEventV2::from(safe_borrowed_event) copies all fields and owns its text. A caller already owning the fields can construct the struct directly and move its String without an extra clone. No borrowed event/string crosses Runtime's FIFO. The borrowed KeyEventV2 is constructed privately only for the synchronous owner encode_key_v2 call.

## Dispatch, limits and receipt

- Resolve and validate the actual controller Lease using the existing command path. KeyV2 neither claims control nor creates an input grant or parallel observer.
- As with existing Key, a controller epoch that opted into stamped recovery must have consumed the current full recovery/layout before KeyV2 encoding or input side effects. Stale/released/replaced actual leases reject independently. This does not force stamping onto legacy epochs.
- Admission validates the owned UTF-8 text payload against MAX_INPUT_PAYLOAD_BYTES, 65536. The safe encoder independently validates scalar/text/value contracts. Some NUL is rejected, not silently turned into the None sentinel.
- The sole Ghostty owner receives physical identity, all ten supported modifier/lock/side bits, the consumed mask, actual action, optional scalar, composing and text unchanged. No Character/Unidentified fallback, mask subtraction, side normalization or layout reconstruction is added.
- MAX_ENCODED_INPUT_BYTES remains 65536. A valid input payload may expand past it in the current keyboard mode; reject before before_input or any native write. Stamped epochs return ResourceLimit, legacy epochs keep their existing validation behavior. No split, trim, multi-write loop, prefix acceptance or automatic retry.
- Use the existing original RequestId and before_input/complete-write receipt path. The actual encoded count is returned only after the existing bounded native write completes. InputKind remains Key with the corresponding existing KeyAction and has_committed_text = !composing && !text.is_empty(). Action conversion is explicit, not a numeric cast.
- Ghostty decides ignored/release/composition behavior from actual modes. Zero encoding returns Input(0), without prewrite hook or PTY write. Release does not follow the controller viewport tail; other nonempty accepted events retain existing follow behavior.
- UI settlement/activity hints are not Runtime fields and are never recaptured here. The existing Service admission book keeps them correlated by the original request.

## Evidence and remaining boundary

Owned conversion tests cover all 176 checked physical values and press/repeat/release under legacy and Kitty modes, plus every defined modifier mask. Real encoder comparisons cover keypad Enter/digit/add versus ordinary identities, lock-state effects, left/right physical modifiers, release and composition. International physical identities are preserved exactly; that does not promise a unique nonempty terminal byte sequence for every key in every protocol mode.

Controlled owned-PTY tests exercise FIFO text ownership after original UI-string mutation, exact RequestId/InputKind/encoded-byte receipt correlation, keypad/lock/side inputs, side-only release, composing zero, invalid scalar, pending stamp and encoded overflow with no hook/write. They use the actual safe producer for reference encoding, not a hand encoder.

Checks passed: Runtime all-targets 159 tests, comprising 99 unit and 60 integration; scoped all-target Clippy with warnings denied; Maurice's safe producer input_v2 suite 6 tests. Six new Runtime unit tests cover the owned type/owner gate and two new owned-PTY tests cover actual FIFO/write correlation. Existing Key, Observer and V1 regression tests remain green.

These are producer/controlled-input tests. Actual GPUI/Windows/macOS/Linux/mobile physical key capture and lossless mapping into the new type remain unaccepted. No Service/protocol/sys/GPUI/mobile files were edited for this Runtime task. The previous audit describes the old API/frontend gaps and must not be read as a claim that adoption is complete.
