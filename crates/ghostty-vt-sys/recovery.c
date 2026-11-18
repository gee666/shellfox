#include "adapter.h"
#define TRY(call) do { int r = (call); if (r != GHOSTTY_SUCCESS) return r; } while (0)

int sf_format_vt(SfVt *vt, uint32_t extras, uint8_t *out, size_t cap, size_t *len) {
    if (vt->callback_error) return vt->callback_error;
    if (extras & ~GHOSTTY_FORMATTER_EXTRAS_ALL) return GHOSTTY_INVALID_VALUE;
    GhosttyFormatter formatter = NULL;
    TRY(ghostty_formatter_terminal_new(NULL, &formatter, vt->terminal));
    GhosttyFormatterFormat format = GHOSTTY_FORMATTER_FORMAT_VT;
    bool disabled = false;
    int r = ghostty_formatter_set(formatter, GHOSTTY_FORMATTER_OPT_EMIT, &format);
    if (!r) r = ghostty_formatter_set(formatter, GHOSTTY_FORMATTER_OPT_TRIM, &disabled);
    if (!r) r = ghostty_formatter_set(formatter, GHOSTTY_FORMATTER_OPT_UNWRAP, &disabled);
    if (!r) r = ghostty_formatter_set(formatter, GHOSTTY_FORMATTER_OPT_EXTRAS, &extras);
    if (!r) r = ghostty_formatter_format_buf(formatter, out, cap, len);
    ghostty_formatter_free(formatter);
    return r;
}

int sf_continuation(SfVt *vt, uint8_t *out, size_t cap, size_t *len) {
    if (vt->callback_error) return vt->callback_error;
    return ghostty_terminal_continuation_buf(vt->terminal, out, cap, len);
}

int sf_recovery_info(SfVt *vt, SfRecoveryInfo *out) {
    if (vt->callback_error) return vt->callback_error;
    GhosttyTerminalScreen screen;
    bool pending_wrap = false;
    size_t scrollback_rows = 0;
    TRY(ghostty_terminal_get(vt->terminal, GHOSTTY_TERMINAL_DATA_COLS, &out->cols));
    TRY(ghostty_terminal_get(vt->terminal, GHOSTTY_TERMINAL_DATA_ROWS, &out->rows));
    TRY(ghostty_terminal_get(vt->terminal, GHOSTTY_TERMINAL_DATA_ACTIVE_SCREEN, &screen));
    if (screen != GHOSTTY_TERMINAL_SCREEN_PRIMARY && screen != GHOSTTY_TERMINAL_SCREEN_ALTERNATE) return GHOSTTY_INVALID_VALUE;
    TRY(ghostty_terminal_get(vt->terminal, GHOSTTY_TERMINAL_DATA_CURSOR_PENDING_WRAP, &pending_wrap));
    TRY(ghostty_terminal_get(vt->terminal, GHOSTTY_TERMINAL_DATA_SCROLLBACK_ROWS, &scrollback_rows));
    out->alternate = screen == GHOSTTY_TERMINAL_SCREEN_ALTERNATE;
    out->pending_wrap = pending_wrap;
    out->scrollback_rows = scrollback_rows;
    return 0;
}
