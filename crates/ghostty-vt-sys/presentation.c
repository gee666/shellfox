#include "presentation.h"
#include <stdlib.h>
#include <string.h>
#define TRY(call) do { int r_ = (call); if (r_ != GHOSTTY_SUCCESS) return r_; } while (0)
#define SF_PRESENTATION_LIMIT (1024u * 1024u)

typedef struct {
    GhosttyTrackedGridRef top, start, end;
    bool follow, rectangle;
    uint8_t kind;
} ViewScreen;
struct SfPresentation {
    ViewScreen screens[2];
    ViewScreen *active;
    uint32_t top, total;
    uint16_t cols, rows, x, y;
    GhosttyColorRgb foreground, background, palette[256];
    GhosttySelection selection;
    bool selected;
    uint32_t *codepoints;
    size_t codepoint_capacity;
};
static void clear_selection(ViewScreen *s) {
    ghostty_tracked_grid_ref_free(s->start);
    ghostty_tracked_grid_ref_free(s->end);
    s->start = NULL; s->end = NULL;
}
int sf_presentation_new(SfPresentation **out) {
    SfPresentation *v = calloc(1, sizeof(*v));
    if (!v) return GHOSTTY_OUT_OF_MEMORY;
    v->screens[0].follow = true; v->screens[1].follow = true;
    *out = v; return 0;
}
void sf_presentation_free(SfPresentation *v) {
    if (!v) return;
    for (size_t i = 0; i < 2; i++) {
        clear_selection(&v->screens[i]);
        ghostty_tracked_grid_ref_free(v->screens[i].top);
    }
    free(v->codepoints); free(v);
}
static GhosttyPoint point(uint16_t x, uint32_t y) {
    GhosttyPoint p = {0}; p.tag = GHOSTTY_POINT_TAG_SCREEN;
    p.value.coordinate.x = x; p.value.coordinate.y = y; return p;
}
static int context(SfVt *vt, SfPresentation *v) {
    if (vt->callback_error) return vt->callback_error;
    GhosttyTerminalScreen screen;
    size_t total = 0;
    TRY(ghostty_terminal_get(vt->terminal, GHOSTTY_TERMINAL_DATA_ACTIVE_SCREEN, &screen));
    if (screen != GHOSTTY_TERMINAL_SCREEN_PRIMARY && screen != GHOSTTY_TERMINAL_SCREEN_ALTERNATE) return GHOSTTY_INVALID_VALUE;
    TRY(ghostty_terminal_get(vt->terminal, GHOSTTY_TERMINAL_DATA_TOTAL_ROWS, &total));
    if (total > UINT32_MAX || total < vt->height) return GHOSTTY_LIMIT_EXCEEDED;
    v->active = &v->screens[screen == GHOSTTY_TERMINAL_SCREEN_ALTERNATE ? 1 : 0];
    v->total = (uint32_t)total; v->cols = vt->cols; v->rows = vt->height;
    uint32_t bottom = v->total - v->rows;
    v->top = bottom;
    if (!v->active->follow) {
        GhosttyPointCoordinate coordinate = {0};
        int r = v->active->top ? ghostty_tracked_grid_ref_point(v->active->top, GHOSTTY_POINT_TAG_SCREEN, &coordinate) : GHOSTTY_NO_VALUE;
        if (r != GHOSTTY_SUCCESS && r != GHOSTTY_NO_VALUE) return r;
        v->top = r == GHOSTTY_NO_VALUE ? 0 : coordinate.y;
        if (v->top > bottom) v->top = bottom;
    }
    return 0;
}
int sf_presentation_scroll(SfVt *vt, SfPresentation *v, intptr_t delta, uint8_t follow) {
    TRY(context(vt, v));
    uint32_t bottom = v->total - v->rows;
    uint32_t top = v->top;
    if (follow) top = bottom;
    else if (delta >= 0) {
        uintptr_t amount = (uintptr_t)delta;
        top += amount > bottom - top ? bottom - top : (uint32_t)amount;
    } else {
        uintptr_t amount = (uintptr_t)(-(delta + 1)) + 1;
        top -= amount > top ? top : (uint32_t)amount;
    }
    GhosttyTrackedGridRef anchor = NULL;
    if (top != bottom) TRY(ghostty_terminal_grid_ref_track(vt->terminal, point(0, top), &anchor));
    ghostty_tracked_grid_ref_free(v->active->top);
    v->active->top = anchor; v->active->follow = top == bottom;
    return 0;
}
static int local_ref(SfVt *vt, SfPresentation *v, uint16_t x, uint16_t y, GhosttyGridRef *out) {
    if (x >= v->cols || y >= v->rows) return GHOSTTY_INVALID_VALUE;
    return ghostty_terminal_grid_ref(vt->terminal, point(x, v->top + y), out);
}
static int selected(SfPresentation *v, GhosttySelection *out) {
    ViewScreen *s = v->active;
    if (!s->start || !s->end) return GHOSTTY_NO_VALUE;
    int r = ghostty_tracked_grid_ref_snapshot(s->start, &out->start);
    if (!r) r = ghostty_tracked_grid_ref_snapshot(s->end, &out->end);
    if (r == GHOSTTY_NO_VALUE) clear_selection(s);
    out->rectangle = s->rectangle;
    return r;
}
int sf_presentation_select(SfVt *vt, SfPresentation *v, const SfSelection *input) {
    TRY(context(vt, v));
    if (!input) { clear_selection(v->active); return 0; }
    GhosttySelection selection = {0};
    int r;
    if (input->kind == 3) r = ghostty_terminal_select_all(vt->terminal, &selection);
    else {
        GhosttyGridRef start = {0};
        TRY(local_ref(vt, v, input->start_x, input->start_y, &start));
        if (input->kind == 0) {
            selection.start = start; selection.rectangle = input->rectangle != 0;
            r = local_ref(vt, v, input->end_x, input->end_y, &selection.end);
        } else if (input->kind == 1) {
            GhosttyTerminalSelectWordOptions opts = GHOSTTY_INIT_SIZED(GhosttyTerminalSelectWordOptions);
            opts.ref = start; r = ghostty_terminal_select_word(vt->terminal, &opts, &selection);
        } else if (input->kind == 2) {
            GhosttyTerminalSelectLineOptions opts = GHOSTTY_INIT_SIZED(GhosttyTerminalSelectLineOptions);
            opts.ref = start; r = ghostty_terminal_select_line(vt->terminal, &opts, &selection);
        } else return GHOSTTY_INVALID_VALUE;
    }
    if (r == GHOSTTY_NO_VALUE) { clear_selection(v->active); return 0; }
    if (r) return r;
    /* Programmatic selections have normalized endpoints, not a fabricated drag. */
    if (input->kind != 0) {
        GhosttySelection normalized = {0};
        TRY(ghostty_terminal_selection_ordered(vt->terminal, &selection, GHOSTTY_SELECTION_ORDER_FORWARD, &normalized));
        selection = normalized;
    }
    /* Resolve both refs before tracking either one. No stale ref crosses a mutation. */
    GhosttyPointCoordinate a, b;
    TRY(ghostty_terminal_point_from_grid_ref(vt->terminal, &selection.start, GHOSTTY_POINT_TAG_SCREEN, &a));
    TRY(ghostty_terminal_point_from_grid_ref(vt->terminal, &selection.end, GHOSTTY_POINT_TAG_SCREEN, &b));
    GhosttyTrackedGridRef first = NULL, last = NULL;
    TRY(ghostty_terminal_grid_ref_track(vt->terminal, point(a.x, a.y), &first));
    r = ghostty_terminal_grid_ref_track(vt->terminal, point(b.x, b.y), &last);
    if (r) { ghostty_tracked_grid_ref_free(first); return r; }
    clear_selection(v->active);
    v->active->start = first; v->active->end = last;
    v->active->rectangle = selection.rectangle;
    v->active->kind = (uint8_t)input->kind; return 0;
}
int sf_presentation_selection(SfVt *vt, SfPresentation *v, SfPresentationSelection *out) {
    TRY(context(vt, v));
    ViewScreen *s = v->active;
    if (!s->start || !s->end) return GHOSTTY_NO_VALUE;
    GhosttyPointCoordinate anchor = {0}, focus = {0};
    /* Read the tracked refs directly. No sorting, viewport projection, selection
       installation, or clearing of pruned endpoints occurs in this query. */
    TRY(ghostty_tracked_grid_ref_point(s->start, GHOSTTY_POINT_TAG_SCREEN, &anchor));
    TRY(ghostty_tracked_grid_ref_point(s->end, GHOSTTY_POINT_TAG_SCREEN, &focus));
    if (anchor.x >= v->cols || focus.x >= v->cols || anchor.y >= v->total || focus.y >= v->total) return GHOSTTY_INVALID_VALUE;
    out->anchor_x = anchor.x; out->anchor_row = anchor.y;
    out->focus_x = focus.x; out->focus_row = focus.y;
    out->view_top_row = v->top; out->total_rows = v->total; out->cols = v->cols;
    out->rectangle = s->rectangle; out->kind = s->kind;
    out->alternate = s == &v->screens[1];
    return 0;
}
int sf_presentation_copy(SfVt *vt, SfPresentation *v, uint8_t *out, size_t cap, size_t *len) {
    TRY(context(vt, v));
    GhosttySelection selection = {0}; TRY(selected(v, &selection));
    GhosttyFormatter formatter = NULL;
    TRY(ghostty_formatter_terminal_new(NULL, &formatter, vt->terminal));
    bool unwrap = true;
    int r = ghostty_formatter_set(formatter, GHOSTTY_FORMATTER_OPT_UNWRAP, &unwrap);
    if (!r) r = ghostty_formatter_set(formatter, GHOSTTY_FORMATTER_OPT_SELECTION, &selection);
    if (!r) r = ghostty_formatter_format_buf(formatter, out, cap, len);
    ghostty_formatter_free(formatter); return r;
}
int sf_presentation_hyperlink(SfVt *vt, SfPresentation *v, uint16_t x, uint16_t y, uint8_t *out, size_t cap, size_t *len) {
    TRY(context(vt, v));
    GhosttyGridRef ref = {0}; TRY(local_ref(vt, v, x, y, &ref));
    return ghostty_grid_ref_hyperlink_uri(&ref, out, cap, len);
}
static int resolve_color(SfPresentation *v, GhosttyStyleColor color, GhosttyColorRgb fallback, uint8_t out[3]) {
    GhosttyColorRgb rgb;
    switch (color.tag) {
    case GHOSTTY_STYLE_COLOR_NONE: rgb = fallback; break;
    case GHOSTTY_STYLE_COLOR_PALETTE: rgb = v->palette[color.value.palette]; break;
    case GHOSTTY_STYLE_COLOR_RGB: rgb = color.value.rgb; break;
    default: return GHOSTTY_INVALID_VALUE;
    }
    out[0] = rgb.r; out[1] = rgb.g; out[2] = rgb.b; return 0;
}
int sf_presentation_begin(SfVt *vt, SfPresentation *v, SfPresentationFrame *out) {
    TRY(context(vt, v));
    SfCursor ignored;
    /* Terminal colors may be unset. Use the shared renderer's public resolved
       defaults, keeping its cache current rather than consuming another cache. */
    TRY(sf_render(vt, &ignored));
    TRY(ghostty_render_state_get(vt->render, GHOSTTY_RENDER_STATE_DATA_COLOR_FOREGROUND, &v->foreground));
    TRY(ghostty_render_state_get(vt->render, GHOSTTY_RENDER_STATE_DATA_COLOR_BACKGROUND, &v->background));
    TRY(ghostty_render_state_get(vt->render, GHOSTTY_RENDER_STATE_DATA_COLOR_PALETTE, &v->palette));
    int r = selected(v, &v->selection);
    if (r && r != GHOSTTY_NO_VALUE) return r;
    v->selected = r == 0;
    out->top_row = v->top; out->total_rows = v->total;
    out->cols = v->cols; out->rows = v->rows;
    uint16_t x, y; bool visible;
    TRY(ghostty_terminal_get(vt->terminal, GHOSTTY_TERMINAL_DATA_CURSOR_X, &x));
    TRY(ghostty_terminal_get(vt->terminal, GHOSTTY_TERMINAL_DATA_CURSOR_Y, &y));
    TRY(ghostty_terminal_get(vt->terminal, GHOSTTY_TERMINAL_DATA_CURSOR_VISIBLE, &visible));
    uint64_t cursor_row = (uint64_t)v->total - v->rows + y;
    out->cursor_visible = visible && cursor_row >= v->top && cursor_row < (uint64_t)v->top + v->rows;
    out->cursor_x = x; out->cursor_y = out->cursor_visible ? (uint16_t)(cursor_row - v->top) : 0;
    v->x = 0; v->y = 0; return 0;
}
int sf_presentation_cell(SfVt *vt, SfPresentation *v, SfPresentationCell *out) {
    if (vt->callback_error) return vt->callback_error;
    if (v->y >= v->rows) return GHOSTTY_NO_VALUE;
    GhosttyGridRef ref = {0}; TRY(local_ref(vt, v, v->x, v->y, &ref));
    size_t len = 0;
    int r = ghostty_grid_ref_graphemes(&ref, v->codepoints, v->codepoint_capacity, &len);
    if (r == GHOSTTY_OUT_OF_SPACE) {
        if (len > SF_PRESENTATION_LIMIT / sizeof(uint32_t)) return GHOSTTY_LIMIT_EXCEEDED;
        uint32_t *next = realloc(v->codepoints, len * sizeof(uint32_t));
        if (!next) return GHOSTTY_OUT_OF_MEMORY;
        v->codepoints = next; v->codepoint_capacity = len;
        r = ghostty_grid_ref_graphemes(&ref, next, len, &len);
    }
    if (r) return r;
    out->codepoints = v->codepoints; out->len = len;
    GhosttyCell cell; GhosttyStyle own, style; GhosttyCellWide wide;
    TRY(ghostty_grid_ref_cell(&ref, &cell)); TRY(ghostty_grid_ref_style(&ref, &own));
    TRY(ghostty_cell_get(cell, GHOSTTY_CELL_DATA_WIDE, &wide));
    TRY(sf_cell_style(cell, own, &style));
    TRY(resolve_color(v, style.fg_color, v->foreground, out->fg));
    TRY(resolve_color(v, style.bg_color, v->background, out->bg));
    sf_color_source(style.fg_color, &out->fg_tag, &out->fg_index);
    sf_color_source(style.bg_color, &out->bg_tag, &out->bg_index);
    out->flags = style.flags;
    out->width = wide == GHOSTTY_CELL_WIDE_WIDE ? 2 : wide == GHOSTTY_CELL_WIDE_NARROW ? 1 : 0;
    bool contains = false, link = false, wrap = false, continuation = false;
    if (v->selected) TRY(ghostty_terminal_selection_contains(vt->terminal, &v->selection, point(v->x, v->top + v->y), &contains));
    TRY(ghostty_cell_get(cell, GHOSTTY_CELL_DATA_HAS_HYPERLINK, &link));
    GhosttyRow row; TRY(ghostty_grid_ref_row(&ref, &row));
    TRY(ghostty_row_get(row, GHOSTTY_ROW_DATA_WRAP, &wrap));
    TRY(ghostty_row_get(row, GHOSTTY_ROW_DATA_WRAP_CONTINUATION, &continuation));
    out->selected = contains; out->has_hyperlink = link;
    out->soft_wrapped = wrap; out->wrap_continuation = continuation;
    if (++v->x == v->cols) { v->x = 0; v->y++; }
    return 0;
}
int sf_cursor_visual(SfVt *vt, SfCursorVisual *out) {
    SfCursor ignored;
    /* Update the shared render cache, not a second cache that consumes its dirty state. */
    TRY(sf_render(vt, &ignored));
    GhosttyCursorStyle shape; bool blinking, visible;
    TRY(ghostty_render_state_get(vt->render, GHOSTTY_RENDER_STATE_DATA_CURSOR_VISUAL_STYLE, &shape));
    TRY(ghostty_render_state_get(vt->render, GHOSTTY_RENDER_STATE_DATA_CURSOR_BLINKING, &blinking));
    TRY(ghostty_terminal_get(vt->terminal, GHOSTTY_TERMINAL_DATA_CURSOR_VISIBLE, &visible));
    TRY(ghostty_terminal_get(vt->terminal, GHOSTTY_TERMINAL_DATA_CURSOR_X, &out->x));
    TRY(ghostty_terminal_get(vt->terminal, GHOSTTY_TERMINAL_DATA_CURSOR_Y, &out->y));
    if (shape < GHOSTTY_CURSOR_STYLE_BAR || shape > GHOSTTY_CURSOR_STYLE_BLOCK_HOLLOW) return GHOSTTY_INVALID_VALUE;
    out->shape = (uint8_t)shape; out->blinking = blinking; out->visible = visible;
    GhosttyColorRgb color;
    int r = ghostty_render_state_get(vt->render, GHOSTTY_RENDER_STATE_DATA_COLOR_CURSOR, &color);
    if (r == GHOSTTY_NO_VALUE) r = ghostty_render_state_get(vt->render, GHOSTTY_RENDER_STATE_DATA_COLOR_FOREGROUND, &color);
    if (r) return r;
    out->color[0] = color.r; out->color[1] = color.g; out->color[2] = color.b;
    GhosttyPoint p = {0}; p.tag = GHOSTTY_POINT_TAG_ACTIVE;
    p.value.coordinate.x = out->x; p.value.coordinate.y = out->y;
    GhosttyGridRef ref = {0}; GhosttyCell cell; GhosttyCellWide wide;
    TRY(ghostty_terminal_grid_ref(vt->terminal, p, &ref));
    TRY(ghostty_grid_ref_cell(&ref, &cell));
    TRY(ghostty_cell_get(cell, GHOSTTY_CELL_DATA_WIDE, &wide));
    out->wide_tail = wide == GHOSTTY_CELL_WIDE_SPACER_TAIL;
    out->width = wide == GHOSTTY_CELL_WIDE_WIDE || out->wide_tail ? 2 : 1;
    return 0;
}
