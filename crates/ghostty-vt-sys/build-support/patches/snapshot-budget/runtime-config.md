# Raw-v1 restoration policy audit

This is a defined Native restoration profile, not universal FullState. Source
and destination need the same declared engine identity. The atomic companion
checks actual source configuration at the cut; a build hash alone is not enough.

## Already in the upstream body

The TERMINAL payload explicitly carries `shell_redraws_prompt` at byte 32.
`Header.init` reads the source value, `Header.encode/decode` preserve the enum,
and native decode assigns `result.flags.shell_redraws_prompt`. `fromDecoded`
does not run the ordinary C constructor's false-policy override. No guessed
shell-redraw option is needed.

Tests cover both the C-constructor false value and true after RIS. Each is
crossed with resize-pull false/true, then source and restored terminals receive
OSC 133 and a width/height resize. Copy text, cursor and policy must agree.
This does not excuse the separate ANSI RIS-reset gap.

Other body fields include dimensions/pixels, current/saved/default modes,
scrolling region, cursor defaults, mouse settings, password state, source
scrollback policies, palettes/colors, title/PWD, previous character, both screens,
saved cursors, charset, keyboard stacks and prompt metadata. Presentation/cache
state is not part of the recovery body.

## Required explicit configuration

The following are not inferred from body data or source cells:

- `runtime_image_storage_limit`, an exact per-screen post-decode limit. Every
  source screen must match it. Different per-screen limits are unsupported.
- `runtime_image_io_policy`, direct-only. File, temporary-file and shared-memory
  image inputs are unsupported by this profile.
- `runtime_resize_pull_scrollback`, required u32 0 or 1. Encoder checks the actual
  source flag. Decoder installs it on the fresh terminal before continuation.
  False is supported for ConPTY; it is never replaced with a guessed true value.
- Cora's exact sized/versioned canonical profile State, captured with the body
  at the consumed input cut, restored before continuation and checked afterwards.

Actual image/decompression admission budgets remain zero. Explicit future image
storage is not an image decode budget. Continuation tracking is configured by
`max_continuation_bytes`; missing source tracking for an unfinished parser fails.

## Canonical unencoded C-stream settings

Raw-v1 admits only the pinned fresh C-stream configuration for settings its body
does not encode. The pure `budgetRawV1ConfigSupported` helper checks source state
without calling effects, allocating or mutating the source. It rejects:

- custom terminfo names, title/checksum reporting, checksum defaults/current
  variants other than zero;
- changed APC per-protocol limits or protocol enablement, including Glyph;
- changed unknown-sequence retention or clipboard-write limits;
- registered glyphs, active drag/drop registrations, clipboard transfers/grants,
  or a pending semantic-failure flag.

The defaults are the defined profile, not a promise that every arbitrary source
configuration can be reconstructed. Source effects function pointers/userdata are
not compared or copied. Tests install a source PTY callback and prove it is not
called by either codec or restored to the passive destination.

A pending APC is explicitly unsupported. Its builder can retain the configuration
that existed when it began, even if today's source settings match defaults.
Neither raw-v1 nor width/join State describes that historical builder policy.
The encoder rejects before publishing; the decoder rejects an old public-encoder
body containing such a continuation, with full cleanup. UTF-8, CSI, OSC and
controls/REP continuations have source-to-body-to-decode/live-suffix tests.

Images, placements/loading, compressed source pages, Legacy layout2 and unknown
records are also unsupported. Source-owned compressed history would need bounded
decompression during encoding, which this raw profile does not implement.

## Intentional destination-local policy

Effects callbacks, PTY replies, clipboard permissions and external I/O callbacks
are not restored. The result starts passive. Future callback installation is a
separate application decision, not evidence of preserved source authorization.
Focus/visibility, selection, viewport, dirty/search flags and UI presentation are
view-local rather than a universal process-state snapshot. Recovery must not
open a URL, grant a permission, emit host replies or invoke source callbacks.

The option layouts changed during this unreleased checkpoint. Equal `sizeof`
values do not make earlier headers compatible: padding can absorb new fields.
Use the exact header digest and ordered-patch/build identity, not size alone.
No final engine acceptance or Legacy parity is claimed by this Native profile.
