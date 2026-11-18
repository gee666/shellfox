/* Experimental format2 ABI. Native002 and original public APIs are unchanged. */
#ifndef GHOSTTY_VT_SHELLFOX_SNAPSHOT_FORMAT2_H
#define GHOSTTY_VT_SHELLFOX_SNAPSHOT_FORMAT2_H
#include <ghostty/vt/terminal.h>
#include <ghostty/vt/shellfox_terminal_profile.h>
#ifdef __cplusplus
extern "C" {
#endif
#define GHOSTTY_SHELLFOX_FORMAT2_ABI 1u
#define GHOSTTY_SHELLFOX_FORMAT2_VERSION 2u
#define GHOSTTY_SHELLFOX_FORMAT2_WIRE_PROFILE 2u
#define GHOSTTY_SHELLFOX_FORMAT2_LAYOUT_ABI 2u
/* Default build returns UNSUPPORTED. Only builtin tests or a distinct explicit
 * shellfox-experimental-format2 build enable this prototype. This does not
 * enable public Legacy construction or imply application/profile acceptance.
 * Exact size/ABI/identity/profile/layout checks. All budgets finite, zero=zero.
 * Source preflight proves ALL page nodes resident before any profile/layout
 * accessor; compressed sources are refused without restoration/mutation.
 * Concrete style/link aliases reject before native reference increments.
 * Managed map/table work is conservatively prepaid from native capacities.
 * Compatibility, the SOLE canonical Profile State and explicit runtime config
 * are inside the single SFOXSN2 blob, without native padding/pointers/size_t.
 */
typedef struct {
  size_t size;
  uint32_t abi_version;
  uint32_t format_version;
  uint32_t wire_profile;
  uint32_t layout_abi;
  uint32_t terminal_profile;
  uint32_t flags;
  uint32_t runtime_resize_pull_scrollback;
  GhosttyString engine_profile;
  uint64_t max_encoded_bytes;
  uint64_t max_allocation_bytes;
  uint64_t max_work_units;
  uint64_t max_physical_slots;
  uint64_t max_rows;
  uint64_t max_text_scalars;
  uint64_t max_text_bytes;
  uint64_t max_continuation_bytes;
  uint32_t max_pages;
  uint32_t max_records;
  uint32_t max_visible_columns;
  uint32_t max_backing_columns;
  uint32_t max_visible_rows;
  uint32_t max_pixel_width;
  uint32_t max_pixel_height;
  uint32_t reserved;
} GhosttyShellfoxSnapshotFormat2Options;
typedef struct {
  size_t size;
  uint32_t abi_version;
  uint32_t reserved;
  uint64_t source_offset;
  uint64_t heap_bytes;
  uint64_t mapped_bytes;
  uint64_t work_units;
  uint64_t physical_slots;
  uint64_t text_scalars;
} GhosttyShellfoxSnapshotFormat2Report;
/* Exclusive source ownership for entire call. No source feed/query/switch,
 * effects callback, source mutation or separate metadata getter. Private charged
 * staging; no output buffer writes on failure, out_written=0. Caller capacity
 * exhaustion returns OUT_OF_SPACE; encoded/continuation policy exhaustion
 * returns LIMIT_EXCEEDED. Slow-integrity/unsupported builds are unsupported.
 * Disjoint outputs
 * must not alias source-owned memory. The scope bounds cumulative OS-rounded
 * heap/pool/direct reservations, not process RSS or later live-input allocations.
 */
GHOSTTY_API GhosttyResult ghostty_shellfox_snapshot_format2_encode_buf(
    GhosttyTerminal source, const GhosttyShellfoxSnapshotFormat2Options *options,
    uint8_t *output, size_t capacity, size_t *out_written,
    GhosttyShellfoxSnapshotFormat2Report *report);
/* Input/options/identity immutable throughout. All framing/layout/count/text/
 * work/alloc limits checked before native restore. Core fresh token stages
 * profile/layout, restores captured join before continuation, verifies after.
 * FINISH/exact EOF required. No partial terminal; *out=NULL on error. Free a
 * successful owned result with ghostty_terminal_free. Destination is passive.
 * No raw1 fallback, new-cell invalid-CP normalization, callback restoration,
 * images/decompression or public Legacy availability follows from these APIs.
 */
GHOSTTY_API GhosttyResult ghostty_shellfox_snapshot_format2_decode_buf(
    const uint8_t *input, size_t length,
    const GhosttyShellfoxSnapshotFormat2Options *options,
    GhosttyTerminal *out, GhosttyShellfoxSnapshotFormat2Report *report);
#ifdef __cplusplus
}
#endif
#endif
