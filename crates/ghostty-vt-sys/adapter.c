#include "adapter.h"
#include <stdlib.h>
#include <string.h>

#define SF_LIMIT (1024u * 1024u)
#define TRY(call) do { int r = (call); if (r != GHOSTTY_SUCCESS) return r; } while (0)

/* The callback never re-enters Ghostty. Bytes are copied before returning. */
static void reply(GhosttyTerminal terminal, void *userdata,
                  const uint8_t *data, size_t len) {
    SfVt *vt = userdata;
    (void)terminal;
    if (vt->callback_error) return;
    if (len > SF_LIMIT - vt->replies_len) {
        vt->callback_error = GHOSTTY_LIMIT_EXCEEDED;
        vt->replies_len = 0;
        return;
    }
    size_t required = vt->replies_len + len;
    if (required > vt->replies_cap) {
        size_t cap = vt->replies_cap ? vt->replies_cap : 4096;
        while (cap < required) cap *= 2;
        uint8_t *next = realloc(vt->replies, cap);
        if (!next) {
            vt->callback_error = GHOSTTY_OUT_OF_MEMORY;
            vt->replies_len = 0;
            return;
        }
        vt->replies = next;
        vt->replies_cap = cap;
    }
    if (len) memcpy(vt->replies + vt->replies_len, data, len);
    vt->replies_len = required;
}

void sf_free(SfVt *vt) {
    if (!vt) return;
    ghostty_render_state_row_cells_free(vt->cells);
    ghostty_render_state_row_iterator_free(vt->rows);
    ghostty_render_state_free(vt->render);
    ghostty_key_encoder_free(vt->encoder);
    ghostty_mouse_encoder_free(vt->mouse);
    ghostty_terminal_free(vt->terminal);
    sf_metadata_free(vt);
    free(vt->replies);
    free(vt->text);
    free(vt);
}

int sf_adapter_storage_new(SfVt *vt) {
    int r = ghostty_render_state_new(NULL, &vt->render);
    if (!r) r = ghostty_render_state_row_iterator_new(NULL, &vt->rows);
    if (!r) r = ghostty_render_state_row_cells_new(NULL, &vt->cells);
    if (!r) r = ghostty_key_encoder_new(NULL, &vt->encoder);
    if (!r) r = ghostty_mouse_encoder_new(NULL, &vt->mouse);
    return r;
}

int sf_new(uint16_t cols, uint16_t rows, SfVt **out) {
    SfVt *vt = calloc(1, sizeof(*vt));
    if (!vt) return GHOSTTY_OUT_OF_MEMORY;
    int r = ghostty_terminal_new(NULL, &vt->terminal, cols, rows);
    /* Match xterm's desktop/mobile default; DECSCUSR may override it later. */
    GhosttyCursorStyle cursor_style = GHOSTTY_CURSOR_STYLE_BLOCK;
    if (!r) r = ghostty_terminal_set(vt->terminal, GHOSTTY_TERMINAL_OPT_DEFAULT_CURSOR_STYLE, &cursor_style);
    /* Tracking must precede the first input, including an unfinished sequence. */
    size_t continuation_limit = 64u * 1024u;
    if (!r) r = ghostty_terminal_set(vt->terminal, GHOSTTY_TERMINAL_OPT_CONTINUATION_MAX_BYTES, &continuation_limit);
    if (!r) r = sf_adapter_storage_new(vt);
    if (!r) r = ghostty_terminal_set(vt->terminal, GHOSTTY_TERMINAL_OPT_USERDATA, vt);
    /* Callback options take the function address itself, not its address. */
    if (!r) r = ghostty_terminal_set(vt->terminal, GHOSTTY_TERMINAL_OPT_WRITE_PTY, (const void *)reply);
    if (!r) r = sf_metadata_register(vt);
    if (r) { sf_free(vt); return r; }
    vt->cols = cols;
    vt->height = rows;
    *out = vt;
    return 0;
}

int sf_feed(SfVt *vt, const uint8_t *data, size_t len) {
    if (vt->callback_error) return vt->callback_error;
    ghostty_terminal_vt_write(vt->terminal, data, len);
#ifdef SHELLFOX_EXPERIMENTAL_NATIVE
    if (!vt->metadata) return sf_status(vt);
#endif
    return vt->callback_error;
}

int sf_resize(SfVt *vt, uint16_t cols, uint16_t rows) {
    if (vt->callback_error) return vt->callback_error;
    TRY(ghostty_terminal_resize(vt->terminal, cols, rows, 0, 0));
    vt->cols = cols;
    vt->height = rows;
    return vt->callback_error;
}

int sf_replies(SfVt *vt, uint8_t *out, size_t *len) {
    if (vt->callback_error) return vt->callback_error;
    *len = vt->replies_len;
    if (out) {
        if (*len) memcpy(out, vt->replies, *len);
        vt->replies_len = 0;
    }
    return 0;
}

int sf_render(SfVt *vt, SfCursor *out) {
#ifdef SHELLFOX_EXPERIMENTAL_NATIVE
    TRY(sf_status(vt));
#else
    if (vt->callback_error) return vt->callback_error;
#endif
    TRY(ghostty_render_state_update(vt->render, vt->terminal));
    GhosttyRenderStateCursor cursor = GHOSTTY_INIT_SIZED(GhosttyRenderStateCursor);
    TRY(ghostty_render_state_get(vt->render, GHOSTTY_RENDER_STATE_DATA_CURSOR, &cursor));
    out->visible = cursor.visible && cursor.viewport_has_value;
    out->x = cursor.viewport_has_value ? cursor.viewport_x : 0;
    out->y = cursor.viewport_has_value ? cursor.viewport_y : 0;
    TRY(ghostty_render_state_get(vt->render, GHOSTTY_RENDER_STATE_DATA_ROW_ITERATOR, &vt->rows));
    vt->x = vt->cols;
    vt->y = 0;
    return 0;
}

static int color(SfVt *vt, GhosttyRenderStateRowCellsData key,
                 GhosttyRenderStateData fallback, uint8_t out[3]) {
    GhosttyColorRgb rgb;
    int r = ghostty_render_state_row_cells_get(vt->cells, key, &rgb);
    if (r == GHOSTTY_NO_VALUE) r = ghostty_render_state_get(vt->render, fallback, &rgb);
    if (r) return r;
    out[0] = rgb.r; out[1] = rgb.g; out[2] = rgb.b;
    return 0;
}

/* Only public getters are used. Packed GhosttyCell bits are not stable ABI. */
int sf_cell(SfVt *vt, SfCell *out) {
    if (vt->callback_error) return vt->callback_error;
    if (vt->x == vt->cols) {
        if (vt->y == vt->height) return GHOSTTY_NO_VALUE;
        if (!ghostty_render_state_row_iterator_next(vt->rows)) return GHOSTTY_INVALID_VALUE;
        TRY(ghostty_render_state_row_get(vt->rows, GHOSTTY_RENDER_STATE_ROW_DATA_CELLS, &vt->cells));
        vt->x = 0;
        vt->y++;
    }
    TRY(ghostty_render_state_row_cells_select(vt->cells, vt->x));
    GhosttyBuffer buffer = { .ptr = vt->text, .cap = vt->text_cap, .len = 0 };
    int r = ghostty_render_state_row_cells_get(vt->cells, GHOSTTY_RENDER_STATE_ROW_CELLS_DATA_GRAPHEMES_UTF8, &buffer);
    if (r == GHOSTTY_OUT_OF_SPACE) {
        if (buffer.len > SF_LIMIT) return GHOSTTY_LIMIT_EXCEEDED;
        uint8_t *next = realloc(vt->text, buffer.len);
        if (!next) return GHOSTTY_OUT_OF_MEMORY;
        vt->text = next; vt->text_cap = buffer.len;
        buffer.ptr = next; buffer.cap = buffer.len;
        r = ghostty_render_state_row_cells_get(vt->cells, GHOSTTY_RENDER_STATE_ROW_CELLS_DATA_GRAPHEMES_UTF8, &buffer);
    }
    if (r) return r;
    out->text = buffer.ptr; out->len = buffer.len;
    TRY(color(vt, GHOSTTY_RENDER_STATE_ROW_CELLS_DATA_FG_COLOR, GHOSTTY_RENDER_STATE_DATA_COLOR_FOREGROUND, out->fg));
    TRY(color(vt, GHOSTTY_RENDER_STATE_ROW_CELLS_DATA_BG_COLOR, GHOSTTY_RENDER_STATE_DATA_COLOR_BACKGROUND, out->bg));
    GhosttyStyle style;
    TRY(ghostty_render_state_row_cells_get(vt->cells, GHOSTTY_RENDER_STATE_ROW_CELLS_DATA_STYLE, &style));
    out->flags = style.flags;
    GhosttyCell raw;
    GhosttyCellWide wide;
    TRY(ghostty_render_state_row_cells_get(vt->cells, GHOSTTY_RENDER_STATE_ROW_CELLS_DATA_RAW, &raw));
    TRY(ghostty_cell_get(raw, GHOSTTY_CELL_DATA_WIDE, &wide));
    out->width = wide == GHOSTTY_CELL_WIDE_WIDE ? 2 : wide == GHOSTTY_CELL_WIDE_NARROW ? 1 : 0;
    bool selected = false, has_hyperlink = false;
    TRY(ghostty_render_state_row_cells_get(vt->cells, GHOSTTY_RENDER_STATE_ROW_CELLS_DATA_SELECTED, &selected));
    TRY(ghostty_cell_get(raw, GHOSTTY_CELL_DATA_HAS_HYPERLINK, &has_hyperlink));
    out->selected = selected;
    out->has_hyperlink = has_hyperlink;
    vt->x++;
    return 0;
}

int sf_scroll(SfVt *vt, intptr_t delta) {
    if (vt->callback_error) return vt->callback_error;
    GhosttyTerminalScrollViewport scroll = {0};
    scroll.tag = GHOSTTY_TERMINAL_SCROLL_VIEWPORT_DELTA;
    scroll.value.delta = delta;
    TRY(ghostty_terminal_scroll_viewport(vt->terminal, scroll));
    return vt->callback_error;
}

int sf_status(SfVt *vt) {
    if (vt->callback_error) return vt->callback_error;
#ifdef SHELLFOX_EXPERIMENTAL_NATIVE
    /* Adopted passive handles have no metadata callbacks to carry failures. */
    if (!vt->metadata) {
        bool failed = false;
        int result = ghostty_terminal_get(vt->terminal, GHOSTTY_TERMINAL_DATA_VT_PROCESSING_ERROR, &failed);
        if (result) return result;
        if (failed) return GHOSTTY_IO_ERROR;
    }
#endif
    return GHOSTTY_SUCCESS;
}

const char *sf_version(void) { return ghostty_version(); }
