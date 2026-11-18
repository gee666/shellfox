#include "adapter.h"
#define TRY(call) do { int r = (call); if (r != GHOSTTY_SUCCESS) return r; } while (0)

int sf_colors(SfVt *vt, const uint8_t *fg, const uint8_t *bg,
              const uint8_t *cursor, const uint8_t *palette) {
    if (vt->callback_error) return vt->callback_error;
    GhosttyColorRgb foreground = {fg[0], fg[1], fg[2]};
    GhosttyColorRgb background = {bg[0], bg[1], bg[2]};
    TRY(ghostty_terminal_set(vt->terminal, GHOSTTY_TERMINAL_OPT_COLOR_FOREGROUND, &foreground));
    TRY(ghostty_terminal_set(vt->terminal, GHOSTTY_TERMINAL_OPT_COLOR_BACKGROUND, &background));
    GhosttyColorRgb cursor_color = {0};
    if (cursor) {
        cursor_color.r = cursor[0]; cursor_color.g = cursor[1]; cursor_color.b = cursor[2];
    }
    TRY(ghostty_terminal_set(vt->terminal, GHOSTTY_TERMINAL_OPT_COLOR_CURSOR, cursor ? &cursor_color : NULL));
    GhosttyColorRgb colors[256];
    if (palette) {
        for (size_t i = 0; i < 256; i++) {
            colors[i].r = palette[3*i]; colors[i].g = palette[3*i+1]; colors[i].b = palette[3*i+2];
        }
    }
    return ghostty_terminal_set(vt->terminal, GHOSTTY_TERMINAL_OPT_COLOR_PALETTE, palette ? colors : NULL);
}
