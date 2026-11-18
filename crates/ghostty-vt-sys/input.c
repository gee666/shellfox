#include "adapter.h"
#define TRY(call) do { int r = (call); if (r != GHOSTTY_SUCCESS) { result = r; goto done; } } while (0)

int sf_key(SfVt *vt, uint32_t key, uint32_t action, uint32_t mods,
           uint32_t consumed_mods, uint32_t unshifted, uint8_t composing,
           const uint8_t *text, size_t text_len,
           uint8_t *out, size_t cap, size_t *len) {
    if (!vt || !len || (text_len && !text) || (cap && !out)) return GHOSTTY_INVALID_VALUE;
    *len = 0;
    if (vt->callback_error) return vt->callback_error;
    /* Validate wide arguments before narrowing or allocating an event. Side
       bits are independent known bits, not evidence a base key is still down. */
    if (key > GHOSTTY_KEY_PASTE || action > GHOSTTY_KEY_ACTION_REPEAT ||
        (mods & ~UINT32_C(0x03ff)) || (consumed_mods & ~UINT32_C(0x03ff)) ||
        composing > 1 || unshifted > UINT32_C(0x10ffff) ||
        (unshifted >= UINT32_C(0xd800) && unshifted <= UINT32_C(0xdfff)))
        return GHOSTTY_INVALID_VALUE;
    GhosttyKeyEvent event = NULL;
    int result = ghostty_key_event_new(NULL, &event);
    if (result) return result;
    TRY(ghostty_key_encoder_set_from_terminal(vt->encoder, vt->terminal));
    TRY(ghostty_key_event_set_key(event, (GhosttyKey)key));
    TRY(ghostty_key_event_set_action(event, (GhosttyKeyAction)action));
    TRY(ghostty_key_event_set_mods(event, (GhosttyMods)mods));
    TRY(ghostty_key_event_set_consumed_mods(event, (GhosttyMods)consumed_mods));
    TRY(ghostty_key_event_set_unshifted_codepoint(event, unshifted));
    TRY(ghostty_key_event_set_composing(event, composing != 0));
    GhosttyString utf8 = { .ptr = text, .len = text_len };
    TRY(ghostty_key_event_set_utf8(event, utf8));
    result = ghostty_key_encoder_encode(vt->encoder, event, out, cap, len);
done:
    /* UTF-8 borrows the Rust argument only until this call returns. */
    ghostty_key_event_free(event);
    return result;
}

int sf_paste(SfVt *vt, uint8_t *text, size_t text_len,
             uint8_t *out, size_t cap, size_t *len) {
    if (vt->callback_error) return vt->callback_error;
    GhosttyTerminalModeConfig mode = {0};
    mode.mode = GHOSTTY_MODE_BRACKETED_PASTE;
    int r = ghostty_terminal_get(vt->terminal, GHOSTTY_TERMINAL_DATA_MODE, &mode);
    if (r) return r;
    if (!mode.value && !ghostty_paste_is_safe((const char *)text, text_len)) return GHOSTTY_REJECTED;
    return ghostty_paste_encode((char *)text, text_len, mode.value, out, cap, len);
}
