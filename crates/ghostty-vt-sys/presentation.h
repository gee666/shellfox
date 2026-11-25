#ifndef SHELLFOX_PRESENTATION_H
#define SHELLFOX_PRESENTATION_H
#include "adapter.h"

typedef struct SfPresentation SfPresentation;
typedef struct {
    uint64_t top_row, total_rows;
    uint16_t cols, rows, cursor_x, cursor_y;
    uint8_t cursor_visible;
} SfPresentationFrame;
typedef struct {
    const uint32_t *codepoints;
    size_t len;
    uint8_t fg[3], bg[3];
    uint16_t flags;
    uint8_t width, selected, has_hyperlink, soft_wrapped, wrap_continuation;
    uint8_t fg_tag, fg_index, bg_tag, bg_index;
} SfPresentationCell;
typedef struct {
    uint16_t x, y;
    uint8_t shape, blinking, visible, width, wide_tail, color[3];
} SfCursorVisual;
typedef struct {
    uint64_t anchor_row, focus_row, view_top_row, total_rows;
    uint16_t anchor_x, focus_x, cols;
    uint8_t rectangle, alternate, kind;
} SfPresentationSelection;
int sf_presentation_selection(SfVt *vt, SfPresentation *view, SfPresentationSelection *out);
int sf_presentation_new(SfPresentation **out);
void sf_presentation_free(SfPresentation *view);
int sf_presentation_scroll(SfVt *vt, SfPresentation *view, intptr_t delta, uint8_t follow);
int sf_presentation_select(SfVt *vt, SfPresentation *view, const SfSelection *input);
int sf_presentation_copy(SfVt *vt, SfPresentation *view, uint8_t *out, size_t cap, size_t *len);
int sf_presentation_hyperlink(SfVt *vt, SfPresentation *view, uint16_t x, uint16_t y, uint8_t *out, size_t cap, size_t *len);
int sf_presentation_begin(SfVt *vt, SfPresentation *view, SfPresentationFrame *out);
int sf_presentation_cell(SfVt *vt, SfPresentation *view, SfPresentationCell *out);
int sf_cursor_visual(SfVt *vt, SfCursorVisual *out);
#endif
