#include "adapter.h"
#define TRY(call) do { int r = (call); if (r != GHOSTTY_SUCCESS) { result = r; goto done; } } while (0)

int sf_mouse_tracking(SfVt *vt, uint8_t *out) {
    if (vt->callback_error) return vt->callback_error;
    bool tracking = false;
    int r = ghostty_terminal_get(vt->terminal, GHOSTTY_TERMINAL_DATA_MOUSE_TRACKING, &tracking);
    if (!r) *out = tracking;
    return r;
}

int sf_mouse(SfVt *vt, const SfMouse *mouse, uint8_t *out, size_t cap, size_t *len) {
    if (vt->callback_error) return vt->callback_error;
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
