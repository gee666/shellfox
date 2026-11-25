#include "adapter.h"
#define TRY(call) do { int r = (call); if (r != GHOSTTY_SUCCESS) { result = r; goto done; } } while (0)

int sf_mouse_tracking(SfVt *vt, uint8_t *out) {
    if (vt->callback_error) return vt->callback_error;
    bool tracking = false;
    int r = ghostty_terminal_get(vt->terminal, GHOSTTY_TERMINAL_DATA_MOUSE_TRACKING, &tracking);
    if (!r) *out = tracking;
    return r;
}

static int mode(SfVt *vt, uint16_t value, bool *out) {
    GhosttyTerminalModeConfig config = { .mode = ghostty_mode_new(value, false) };
    int r = ghostty_terminal_get(vt->terminal, GHOSTTY_TERMINAL_DATA_MODE, &config);
    if (!r) *out = config.value;
    return r;
}

/* As in Ghostty: without mouse reporting the alternate screen has no history,
 * so the wheel becomes cursor keys while the application allows it (1007). */
int sf_alternate_scroll(SfVt *vt, uint8_t *out) {
    if (vt->callback_error) return vt->callback_error;
    bool tracking = false, enabled = false;
    GhosttyTerminalScreen screen = GHOSTTY_TERMINAL_SCREEN_PRIMARY;
    int r = ghostty_terminal_get(vt->terminal, GHOSTTY_TERMINAL_DATA_MOUSE_TRACKING, &tracking);
    if (!r) r = ghostty_terminal_get(vt->terminal, GHOSTTY_TERMINAL_DATA_ACTIVE_SCREEN, &screen);
    if (!r) r = mode(vt, 1007, &enabled);
    if (!r) *out = !tracking && screen == GHOSTTY_TERMINAL_SCREEN_ALTERNATE && enabled;
    return r;
}

/* Wheel up or down as one cursor key, honoring application cursor keys. */
static int alternate_scroll_key(SfVt *vt, const SfMouse *mouse, uint8_t *out, size_t cap, size_t *len) {
    uint8_t active = 0;
    bool application = false;
    int r = sf_alternate_scroll(vt, &active);
    if (r || !active) return r;
    r = mode(vt, 1, &application);
    if (r) return r;
    *len = 3;
    if (cap < 3) return GHOSTTY_OUT_OF_SPACE;
    out[0] = 0x1b;
    out[1] = application ? 'O' : '[';
    out[2] = mouse->button == 4 ? 'A' : 'B';
    return 0;
}

int sf_mouse(SfVt *vt, const SfMouse *mouse, uint8_t *out, size_t cap, size_t *len) {
    if (vt->callback_error) return vt->callback_error;
    *len = 0;
    if (mouse->action == 0 && (mouse->button == 4 || mouse->button == 5)) {
        int r = alternate_scroll_key(vt, mouse, out, cap, len);
        if (r || *len) return r;
    }
    GhosttyMouseEvent event = NULL;
    int result = ghostty_mouse_event_new(NULL, &event);
    if (result) return result;
    TRY(ghostty_mouse_encoder_set_from_terminal(vt->mouse, vt->terminal));
    GhosttyMouseEncoderSize size = GHOSTTY_INIT_SIZED(GhosttyMouseEncoderSize);
    size.cell_width = mouse->cell_width;
    size.cell_height = mouse->cell_height;
    size.screen_width = size.cell_width * vt->cols;
    size.screen_height = size.cell_height * vt->height;
    TRY(ghostty_mouse_encoder_set(vt->mouse, GHOSTTY_MOUSE_ENCODER_OPT_SIZE, &size));
    bool pressed = mouse->any_button_pressed != 0;
    TRY(ghostty_mouse_encoder_set(vt->mouse, GHOSTTY_MOUSE_ENCODER_OPT_ANY_BUTTON_PRESSED, &pressed));
    TRY(ghostty_mouse_event_set_action(event, (GhosttyMouseAction)mouse->action));
    if (mouse->button) {
        TRY(ghostty_mouse_event_set_button(event, (GhosttyMouseButton)mouse->button));
    } else {
        TRY(ghostty_mouse_event_clear_button(event));
    }
    TRY(ghostty_mouse_event_set_mods(event, (GhosttyMods)mouse->mods));
    GhosttyMousePosition position = { .x = mouse->x, .y = mouse->y };
    TRY(ghostty_mouse_event_set_position(event, position));
    result = ghostty_mouse_encoder_encode(vt->mouse, event, out, cap, len);
done:
    ghostty_mouse_event_free(event);
    return result;
}
