#include "adapter.h"
#define TRY(call) do { int r = (call); if (r != GHOSTTY_SUCCESS) return r; } while (0)

static int viewport_ref(SfVt *vt, uint16_t x, uint16_t y, GhosttyGridRef *out) {
    if (x >= vt->cols || y >= vt->height) return GHOSTTY_INVALID_VALUE;
    GhosttyPoint point = {0};
    point.tag = GHOSTTY_POINT_TAG_VIEWPORT;
    point.value.coordinate.x = x;
    point.value.coordinate.y = y;
    return ghostty_terminal_grid_ref(vt->terminal, point, out);
}

int sf_selection(SfVt *vt, const SfSelection *input) {
    if (vt->callback_error) return vt->callback_error;
    if (!input) return ghostty_terminal_set(vt->terminal, GHOSTTY_TERMINAL_OPT_SELECTION, NULL);
    GhosttySelection selection = {0};
    int r;
    if (input->kind == 3) {
        r = ghostty_terminal_select_all(vt->terminal, &selection);
    } else {
        GhosttyGridRef start = {0};
        TRY(viewport_ref(vt, input->start_x, input->start_y, &start));
        switch (input->kind) {
        case 0:
            selection.start = start;
            TRY(viewport_ref(vt, input->end_x, input->end_y, &selection.end));
            selection.rectangle = input->rectangle != 0;
            r = GHOSTTY_SUCCESS;
            break;
        case 1: {
            GhosttyTerminalSelectWordOptions options = GHOSTTY_INIT_SIZED(GhosttyTerminalSelectWordOptions);
            options.ref = start;
            r = ghostty_terminal_select_word(vt->terminal, &options, &selection);
            break;
        }
        case 2: {
            GhosttyTerminalSelectLineOptions options = GHOSTTY_INIT_SIZED(GhosttyTerminalSelectLineOptions);
            options.ref = start;
            r = ghostty_terminal_select_line(vt->terminal, &options, &selection);
            break;
        }
        default: return GHOSTTY_INVALID_VALUE;
        }
    }
    if (r == GHOSTTY_NO_VALUE) return ghostty_terminal_set(vt->terminal, GHOSTTY_TERMINAL_OPT_SELECTION, NULL);
    if (r) return r;
    /* Install immediately. The terminal converts the refs to tracked state. */
    return ghostty_terminal_set(vt->terminal, GHOSTTY_TERMINAL_OPT_SELECTION, &selection);
}

int sf_copy_selection(SfVt *vt, uint8_t *out, size_t cap, size_t *len) {
    if (vt->callback_error) return vt->callback_error;
    GhosttySelection selection = {0};
    TRY(ghostty_terminal_get(vt->terminal, GHOSTTY_TERMINAL_DATA_SELECTION, &selection));
    GhosttyFormatter formatter = NULL;
    TRY(ghostty_formatter_terminal_new(NULL, &formatter, vt->terminal));
    /* Plain text with trimmed trailing whitespace; join soft-wrapped lines.
       No terminal mutation or borrowed grid reference survives this call. */
    bool unwrap = true;
    int r = ghostty_formatter_set(formatter, GHOSTTY_FORMATTER_OPT_UNWRAP, &unwrap);
    if (!r) r = ghostty_formatter_set(formatter, GHOSTTY_FORMATTER_OPT_SELECTION, &selection);
    if (!r) r = ghostty_formatter_format_buf(formatter, out, cap, len);
    ghostty_formatter_free(formatter);
    return r;
}

int sf_hyperlink(SfVt *vt, uint16_t x, uint16_t y, uint8_t *out, size_t cap, size_t *len) {
    if (vt->callback_error) return vt->callback_error;
    GhosttyGridRef ref = {0};
    TRY(viewport_ref(vt, x, y, &ref));
    return ghostty_grid_ref_hyperlink_uri(&ref, out, cap, len);
}
