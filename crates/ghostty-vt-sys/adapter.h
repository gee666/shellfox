#ifndef SHELLFOX_VT_ADAPTER_H
#define SHELLFOX_VT_ADAPTER_H
#include <ghostty/vt.h>
#include <stdint.h>
#include <stddef.h>
#include "metadata.h"

typedef struct SfVt {
    GhosttyTerminal terminal;
    GhosttyRenderState render;
    GhosttyRenderStateRowIterator rows;
    GhosttyRenderStateRowCells cells;
    GhosttyKeyEncoder encoder;
    GhosttyMouseEncoder mouse;
    SfMetadata *metadata;
    uint8_t *replies, *text;
    size_t replies_len, replies_cap, text_cap;
    int callback_error;
    uint16_t cols, height, x, y;
} SfVt;
typedef struct {
    uint16_t x, y;
    uint8_t visible;
} SfCursor;
typedef struct {
    const uint8_t *text;
    size_t len;
    uint8_t fg[3], bg[3];
    uint16_t flags;
    uint8_t width, selected, has_hyperlink;
    /* GhosttyStyleColorTag of each color, and its palette index. */
    uint8_t fg_tag, fg_index, bg_tag, bg_index;
} SfCell;
typedef struct {
    uint32_t action, button, mods, cell_width, cell_height;
    float x, y;
    uint8_t any_button_pressed;
} SfMouse;
typedef struct {
    uint32_t kind;
    uint16_t start_x, start_y, end_x, end_y;
    uint8_t rectangle;
} SfSelection;
typedef struct {
    uint64_t scrollback_rows;
    uint16_t cols, rows;
    uint8_t alternate, pending_wrap;
} SfRecoveryInfo;
/* The cell's style, with a background stored in the cell itself. */
int sf_cell_style(GhosttyCell cell, GhosttyStyle style, GhosttyStyle *out);
/* A style color's tag and palette index; RGB values come from the resolved color. */
void sf_color_source(GhosttyStyleColor color, uint8_t *tag, uint8_t *index);
int sf_metadata_register(SfVt *vt);
void sf_metadata_free(SfVt *vt);
int sf_metadata_peek(SfVt *vt, SfMetadataView *out);
int sf_metadata_ack(SfVt *vt, uint64_t version);
int sf_format_vt(SfVt *vt, uint32_t extras, uint8_t *out, size_t cap, size_t *len);
int sf_continuation(SfVt *vt, uint8_t *out, size_t cap, size_t *len);
int sf_recovery_info(SfVt *vt, SfRecoveryInfo *out);
int sf_status(SfVt *vt);
int sf_colors(SfVt *vt, const uint8_t *fg, const uint8_t *bg,
              const uint8_t *cursor, const uint8_t *palette);
int sf_mouse_tracking(SfVt *vt, uint8_t *out);
int sf_alternate_scroll(SfVt *vt, uint8_t *out);
int sf_mouse(SfVt *vt, const SfMouse *mouse, uint8_t *out, size_t cap, size_t *len);
int sf_selection(SfVt *vt, const SfSelection *selection);
int sf_copy_selection(SfVt *vt, uint8_t *out, size_t cap, size_t *len);
int sf_hyperlink(SfVt *vt, uint16_t x, uint16_t y, uint8_t *out, size_t cap, size_t *len);
int sf_viewport_rows(SfVt *vt, uint8_t *flags, size_t count);
int sf_adapter_storage_new(SfVt *vt);
int sf_new(uint16_t cols, uint16_t rows, SfVt **out);
void sf_free(SfVt *vt);
int sf_feed(SfVt *vt, const uint8_t *data, size_t len);
int sf_resize(SfVt *vt, uint16_t cols, uint16_t rows);
int sf_replies(SfVt *vt, uint8_t *out, size_t *len);
int sf_render(SfVt *vt, SfCursor *cursor);
int sf_cell(SfVt *vt, SfCell *out);
int sf_key(SfVt *vt, uint32_t key, uint32_t action, uint32_t mods,
           uint32_t consumed_mods, uint32_t unshifted, uint8_t composing,
           const uint8_t *text, size_t text_len,
           uint8_t *out, size_t cap, size_t *len);
int sf_kitty_key_flags(SfVt *vt, uint8_t *out);
int sf_paste(SfVt *vt, uint8_t *text, size_t text_len,
             uint8_t *out, size_t cap, size_t *len);
int sf_scroll(SfVt *vt, intptr_t delta);
const char *sf_version(void);
#endif
