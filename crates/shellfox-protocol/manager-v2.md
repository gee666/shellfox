# Manager-v2 implementation checkpoint

This checkpoint contains actual compiled protocol/client code and golden examples, not an enabled production route or complete native input acceptance. Default general Client remains interactive v1. Existing observer-v2 strict models/goldens and immutable 860d artifacts are unchanged.

## Modules and model boundary

- `shellfox_protocol::manager_v2` owns the separate closed profile, binding, pair, attachment, control/layout, input, error, event and 43-verb models.
- `shellfox_client::manager_v2` exports ManagerClient, ManagerCursor, UploadCursor, AcceptedContext and typed errors.
- `tests/fixtures/manager_v2` contains exact JSON bodies; methods.json covers all43 mappings. These are illustrative protocol examples, not native launch/privilege measurements or process captures.
- Compatible presence-aware Fact/Settings/catalog/session/file/statistics leaves are reused from observer-v2. No legacy settings defaults are inserted.
- A private generic v2 reply codec and private shared client lifecycle/transport/delivery trackers are used by both profiles. The observer client was not copied and its wire acceptance was not widened.

Same five v2 POST paths, no new verbs/Go route/header or local native trust API. Pair profile is exactly manager-v2; inputFormat is manager-input-v1. PairSelection has fixed devicePolicy control-full-account-v1 and devicePermission write. The offer accepts no permission or filesystem-root fields. Permission does not create a controller lease.

## Real associations and manager-specific fields

ControlRef and LayoutRef contain exact tab/generation, independent opaque IDs and positive safe epochs. An owned attachment validates that its delivery and control share identity and actual controller epoch. Busy/notReady/intrinsicReadOnly/sourcePolicyDenied carry input:null and layout:null. There is no success deviceDenied state. Revoked/unapproved callers receive an authentication/permission error.

Attachment::Attached contains Box<Attached>, serialized flat under kind:attached. This is JSON indirection only, not another Runtime. The backend must associate DeliveryRef with EITHER the actual controller lease OR an actual observer lease, never both or a fake acknowledged stream. A separate ControlRef maps to that SAME actual controller, current authenticated peer/connection and native policy epoch.

Attach requires control:none|ifFree. It never takes over automatically. Explicit takeTerminalControl requires mode:takeover and returns a complete replacement attachment/recovery. Promotion/control loss retires old refs and privacy-sensitive queued delivery. Continuing observation requires explicit fresh delivery/recovery, not an invisible role conversion.

Manager Recovery has the established field names but native geometry bounds: axes1..32767, product<=1048576 absolute hard cap. The server additionally checks the original source's configured GridLimits. Observer2..500 limits do not change. RecoverStamped must produce bytes/capability/last DELIVERY sequence/actual grid/canonical revision/opaque recovery ID at ONE barrier. It is FormattedActiveBufferV1, not FullState.

Manager recovery ACK REQUIRES layoutRevision in addition to the recovery UUID and sequence. It maps privately to exact ControllerLayoutAck or ObserverLayoutAck by the real association. Data ACK cannot consume a recovery or geometry barrier. No standalone wire layout ACK grants resize permission.

Resize requires ControlRef, LayoutRef and expectedLayoutRevision. A real geometry change requires full fresh recovery before later input/delivery. Identical dimensions do not manufacture revisions. Manager layout events carry lastSequence for the ordered byte boundary.

Detach of a controller-backed delivery requires releasing its real control association; leaving an unconsumed hidden controller stream is not supported. Client cursor builders require the matching releaseControl reference. Observer detach releases only its real observer. Neither closes a process. CloseTab/DeleteSession remain independent manager permissions and must stop/drain the original source before runtime/metadata removal.

## Input and receipts

writeTerminal is the closed key/paste/base64-byte/StructuredMouse union in input.rs. Key/paste/mouse encoding belongs to the owner. No encoded-mouse string or bytes origin:mouse is accepted. Base64 bytes preserve high bytes and NUL and are admitted only through checked interactive control; automation raw stdin/local RawInput APIs are not widened.

InputHints freezes userInitiated, activityHint and tri-state settledAt at FIFO admission. SettlementCapture::Unavailable represents omission and must map to host CapturedSettlement::Unavailable, never Legacy or a late marker lookup. Explicit null and an actual timestamp remain distinct. Hints are metadata, not authority or privilege measurement.

AcceptedInput correlates inputId and actual matched receipt kind/count. It contains no input, password, source identity or replacement grant. Zero means actual ignored encoding, not a failed write disguised as success. ManagerClient checks byte receipt counts and input ID/kind against the original typed request. Unknown/partial I/O is an explicit non-retryable error, not acceptedBytes:0.

ManagerCursor requires native recovery consumption AND an exact successful recovery ACK before admit_input, resize or uploads. It captures original hints in a bounded16-entry context book without retaining secret encoded input. Deterministic pre-admission Full retains the SAME frozen request/context. Known accepted results can be surfaced after control loss without reinstating refs; uncertain input is not canceled as safely unwritten or replayed.

Uploads retain the exact ControlRef through begin/chunk/finish. Current control/readiness is rechecked for every side effect; cleanup retains the original upload/control identity after revocation. Finish never pastes a returned path. Path insertion is a separate explicit input with the original captured intent.

## Payload versus encoded budget, current parity gap

MAX_INPUT_PAYLOAD_BYTES and MAX_ENCODED_INPUT_BYTES are separate named limits, both currently65536. MAX_INPUT_BYTES is a compatibility name for the payload limit, not an assertion that all valid payloads fit after owner encoding.

A valid65536-byte Paste/key text may expand beyond65536 under actual terminal modes. Runtime must reject the COMPLETE encoded result before writer, live-input hook, activity supplier or metadata effects. Surface manager ResourceLimit{limit:encodedInputBytes}; do not truncate, split, emit a fake zero receipt or call a partial prefix complete. Native InputWriter retains its existing64KiB write cap.

No multi-write expansion is implemented. Any future expansion requires review of bounded asynchronous sequencing, confirmed-prefix/uncertain-write receipts, activity/settlement once-only semantics, control-loss fencing and global queued-byte budgets. It must not block the Runtime owner loop.

Protocol/client tests distinguish valid payload and encoded ResourceLimit. Actual mode-enabled bracketed-paste boundary and zero-side-effect proof belongs to the Runtime/source tests using the real pinned encoder; a protocol fixture is not that proof. Until coordinated source tests pass, this remains a parity gap, not final acceptance.

## Full KeyV2 model checkpoint; canonical conversion and capture are not frozen

Canonical Input::Key now uses the full KeyEventV2 model. The provisional Key/NamedKey/Character/KeyAction/KeyEvent path and keys.rs file are removed. Physical identity is the closed numeric176 enum, not an ASCII or platform-string guess. Genuine Unidentified with separate committed UTF-8/composition remains valid, but known native keys must not collapse to it.

The pinned public vt1.0.1 key/event.h contains176 physical keys, including international17..19, Convert/Kana/NonConvert65..67, NumLock79 and distinct numpad80..119. Ordinary Enter58 is not NumpadEnter97. It supports Caps/Num bits4/5 and right-side bits6..9. Header SHA256 is 4a615f9864ae11bafa06cce7493aa6537804750ed4a6fda8b3f962ea348a8bc9 from the retained authenticated source archive.

Current wrapper/native gaps: safe Key/Modifiers exposes only a subset/four flags; mobile Rust rejects modifiers>15 and sets composing:false; Android collapses numpad Enter and falls back from layout Unicode to ASCII/Unidentified; iOS similarly derives fallback identity from characters and omits lock/side flags. These are NOT lossless full-input mappings.

The compiled additive producer API is now matched by manager PhysicalKey, KeyActionV2, ModifiersV2, UnshiftedScalar and KeyEventV2. Wire physical IDs are the closed numeric0..175 set; active/consumed modifiers are required known-bit u32 masks checked before u16 narrowing. Text is required UTF-8 with the exact public guide: reject C0/DEL/F700..F8FF, preserve C1. Required nullable scalar distinguishes absence from explicitly forbidden NUL. All defined bit patterns survive without side/base or consumed-subset inference, including release.

The canonical key branch/fixture flip was coordinated only after the owned Runtime KeyV2 producer compiled and Musa agreed the checked conversion. Service converts to OwnedKeyEventV2 and Command::KeyV2, never the old safe KeyEvent. All required fields survive; text is moved as owned UTF-8 and borrowed only for actual owner encoding. The mouse four-flag Modifiers type was moved to mouse.rs; Mouse DTO is unchanged.

The protocol tests compare all176 against the producer's single source table as supporting evidence without any sys dependency. The actual compiled conversion proof belongs at Service's existing production-dependency junction plus Maurice's public-header C proof; no Service-lib relink is authorized here. Eva/native owners still must establish actual OS/GPUI capture availability. No native profile3 activation or complete capture acceptance is claimed.

## Remaining integration gates

Actual backend peer/approval/native-purpose/real-lease records remain authority, not JSON IDs, source tags or PairSelection Write. Source event routing must handle actual controller layout/revocation, not ignore it. Guest privilege remains Unknown unless independently measured; no admin override is introduced.

ManagerClient is explicit. Its validated pre-pair legacy offer never pairs/attaches/controls automatically and never chooses observer-v2 as an interactive fallback. General v1 control remains available until the complete manager factory/native pipeline and matching artifacts are accepted.

No Go, backend, Runtime, native creator/header, mobile capture, framework or dependency edits accompany this protocol/client checkpoint. Production advertisement, native ABI3 proposal and matching builds remain separately coordinated. No old artifact is relabeled.
