#ifndef SHELLFOX_MOBILE_H
#define SHELLFOX_MOBILE_H
#include <stddef.h>
#include <stdint.h>
#ifdef __cplusplus
extern "C" {
#endif
#define SFM_ABI_VERSION 1
#define SFM_MAX_CHUNK_BYTES 262144
#define SFM_MAX_QUEUE_BYTES 4194304
#define SFM_MAX_QUEUE_CHUNKS 64
#define SFM_MAX_INPUT_BYTES 65536
/* All functions return a status, never a thrown exception. BUSY accepts nothing.
 * Handles are opaque IDs, not pointers. Never expose pointers to JavaScript.
 * create/pump/frame/resize/input/close MUST use the same native serial thread.
 * submit and progress can run on transport workers. Do not call from JS for
 * each output chunk. Close rejects further submissions, including racing ones.
 * All pointer+length arguments must identify readable memory for the call;
 * output pointers must identify writable memory. NULL is legal only at len=0.
 * Callbacks are synchronous, must not throw, and retain no borrowed pointer.
 * Copy callback data before dispatching to another thread. Except for input
 * callbacks as documented below, callbacks must not reenter this API.
 */
typedef uint64_t SfmHandle;
typedef int32_t SfmStatus;
enum { SFM_OK=0, SFM_BUSY=1, SFM_INVALID=2, SFM_STALE=3,
       SFM_WRONG_THREAD=4, SFM_ENGINE=5, SFM_PANIC=6 };
typedef void (*SfmBytesFn)(void *context, const uint8_t *bytes, size_t length);
/* Additive input callback. The legacy SfmBytesFn signature is unchanged. */
typedef void (*SfmInputExFn)(void *context, const uint8_t *bytes, size_t length,
                             uint8_t user_initiated);
typedef void (*SfmErrorFn)(void *context, SfmStatus status);
typedef struct {
  uint64_t accepted_sequence;
  uint64_t consumed_sequence;
  uint32_t queued_bytes;
  uint32_t queued_chunks;
} SfmProgress;
/* RGB packed 0x00RRGGBB. flags: bold=1 italic=2 underline=4 inverse=8
 * faint=16 invisible=32 strike=64 selected=128 hyperlink=256.
 * Text is UTF-8, NOT NUL terminated. width=0 is a wide-cell spacer.
 */
typedef struct {
  const uint8_t *text;
  size_t text_length;
  uint32_t foreground;
  uint32_t background;
  uint16_t flags;
  uint8_t width;
} SfmCell;
typedef struct {
  uint16_t cols, rows, cursor_x, cursor_y;
  uint8_t cursor_visible;
  const SfmCell *cells;
  size_t cell_count;
  uint64_t consumed_sequence;
} SfmFrame;
typedef void (*SfmFrameFn)(void *context, const SfmFrame *frame);
uint32_t sfm_abi_version(void);
/* Static NUL-terminated sanitized message. Never free it. */
const char *sfm_status_message(SfmStatus status);
/* cols/rows 1..512; product <=65536; at most 32 live engines per process. */
SfmStatus sfm_terminal_create(uint16_t cols, uint16_t rows, SfmHandle *out);
SfmStatus sfm_terminal_close(SfmHandle handle);
/* sequence starts at 1, increments by one. Rejected submissions do not advance.
 * A sequence is a local delivery ticket, NOT the remote host byte cursor.
 * Apply shared-client epoch/cursor validation before submit. No silent drops.
 */
SfmStatus sfm_terminal_submit(SfmHandle handle, uint64_t sequence,
                            const uint8_t *bytes, size_t length);
SfmStatus sfm_terminal_progress(SfmHandle handle, SfmProgress *out);
/* Consumes <=512KiB/tick. Errors poison the view and invoke error callback.
 * consumed_sequence is credit only after Ghostty accepted the entire chunk.
 * Mobile is a mirror: host owns terminal query replies. Never echo mirror
 * replies to the remote host. Parsing always drains/discards those replies.
 */
SfmStatus sfm_terminal_pump(SfmHandle handle, SfmErrorFn error, void *context);
/* Native renderer only, on its serial owner thread. Never bridge cells to JS. */
SfmStatus sfm_terminal_frame(SfmHandle handle, SfmFrameFn frame, void *context);
SfmStatus sfm_terminal_resize(SfmHandle handle, uint16_t cols, uint16_t rows);
/* OWNER THREAD ONLY. Replaces parser and selection, clears queued bytes, resets
 * accepted/consumed local tickets to zero. Also recovers a poisoned engine.
 * FIRST fence the old shared-client generation/lease and stop its submissions
 * and ACKs. THEN reset here on HandlerThread/NSThread. Only after success may
 * native client bind/attach install recovery and resume with local ticket 1.
 * This function cannot identify stale transport callbacks by itself.
 * A failed reset leaves the previous engine and queue unchanged.
 */
SfmStatus sfm_terminal_reset(SfmHandle handle, uint16_t cols, uint16_t rows);
/* Strict UTF-8 JSON, <=16384 bytes; ../schema/configure.schema.json.
 * Applies parser defaults using Terminal::set_colors. Owner thread only.
 * Native renderer uses configured cursor/selection colors and background too.
 * Configure once on creation and after theme changes. Reset preserves config.
 */
SfmStatus sfm_terminal_configure(SfmHandle handle, const uint8_t *json, size_t length);
/* Strict UTF-8 JSON described by ../schema/input.schema.json. Ghostty encodes
 * keyboard-editor edits, keys, paste and mouse; editorReset never sends bytes.
 * Callback bytes are borrowed and must enter the shared client's
 * ownership/epoch-checked input operation. Native text input owns marked text;
 * report only committed text, and do not also send a hardware key's text.
 * Input callbacks run AFTER all engine borrows and queue locks are released.
 * They may enter the client API or query terminal progress/frame/mouse tracking
 * on this owner thread. Do not recursively encode input or reset/close the engine.
 */
SfmStatus sfm_terminal_input(SfmHandle handle, const uint8_t *json, size_t length,
                           SfmBytesFn encoded, void *context);
/* Same validation, owner thread, borrowed bytes and post-engine-borrow callback
 * lifetime as input. Callback runs only for nonempty encoded bytes; bytes may
 * contain NUL, so always honor length rather than strlen.
 * Rust derives user_initiated as 0 or 1 from the validated input variant:
 * editor edits, nonempty paste, key press/repeat, mouse press/release/wheel and
 * drag=1; key release and mouse motion with no pressed button=0.
 * Forward this hint unchanged to sfm_client_terminal_write_ex. Native and JS
 * must not reclassify bytes or accept a caller-provided JSON hint. The host's
 * meaningful_input policy remains authoritative for passive terminal reports.
 */
SfmStatus sfm_terminal_input_ex(SfmHandle handle, const uint8_t *json, size_t length,
                              SfmInputExFn encoded, void *context);
/* Native gesture/clipboard APIs. Every operation runs on the engine owner.
 * Selection kind: 0=clear, 1=inclusive range, 2=word, 3=line, 4=all.
 * Points are zero-based viewport cells; rectangle is 0 or 1.
 * Clipboard/URI callbacks copy UTF-8 text only, with an empty value for none.
 * URI is UNTRUSTED. Native UI must require user intent and validate the scheme
 * before launching it. Rust never opens URLs or writes the OS clipboard.
 */
typedef struct {
  uint32_t kind;
  uint16_t start_x, start_y, end_x, end_y;
  uint8_t rectangle;
} SfmSelection;
SfmStatus sfm_terminal_scroll(SfmHandle handle, int32_t rows);
SfmStatus sfm_terminal_select(SfmHandle handle, SfmSelection selection);
SfmStatus sfm_terminal_copy(SfmHandle handle, SfmBytesFn text, void *context);
SfmStatus sfm_terminal_hyperlink(SfmHandle handle, uint16_t x, uint16_t y,
                               SfmBytesFn uri, void *context);
/* Resolves a plaintext file target in the owner viewport, including wrapped rows. */
SfmStatus sfm_terminal_file_link(SfmHandle handle, uint16_t x, uint16_t y,
                                SfmBytesFn target, void *context);
SfmStatus sfm_terminal_mouse_tracking(SfmHandle handle, uint8_t *out);
/* Whether wheel steps go to the application: mouse reports, or Up/Down keys
 * in the alternate screen. Otherwise scroll with sfm_terminal_scroll. */
SfmStatus sfm_terminal_wheel_reporting(SfmHandle handle, uint8_t *out);
/* The wrapper currently exposes cursor position/visibility, not shape or blink.
 * Render a steady block cursor until that public contract is extended. */
#ifdef __cplusplus
}
#endif
#endif
