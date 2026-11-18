/* Opt-in test-only probe, compiled against the selected real public library. */
#include "../adapter.h"
#include <limits.h>
#include <string.h>
_Static_assert(sizeof(GhosttyKey) == 4, "key enum width");
_Static_assert(sizeof(GhosttyKeyAction) == 4, "action enum width");
_Static_assert(sizeof(GhosttyMods) == 2, "mods width");
_Static_assert(GHOSTTY_MODS_SHIFT == 1 && GHOSTTY_MODS_CTRL == 2 && GHOSTTY_MODS_ALT == 4 && GHOSTTY_MODS_SUPER == 8, "base bits");
_Static_assert(GHOSTTY_MODS_CAPS_LOCK == 16 && GHOSTTY_MODS_NUM_LOCK == 32, "lock bits");
_Static_assert(GHOSTTY_MODS_SHIFT_SIDE == 64 && GHOSTTY_MODS_CTRL_SIDE == 128 && GHOSTTY_MODS_ALT_SIDE == 256 && GHOSTTY_MODS_SUPER_SIDE == 512, "side bits");
_Static_assert(GHOSTTY_KEY_UNIDENTIFIED == 0, "Unidentified");
_Static_assert(GHOSTTY_KEY_BACKQUOTE == 1, "Backquote");
_Static_assert(GHOSTTY_KEY_BACKSLASH == 2, "Backslash");
_Static_assert(GHOSTTY_KEY_BRACKET_LEFT == 3, "BracketLeft");
_Static_assert(GHOSTTY_KEY_BRACKET_RIGHT == 4, "BracketRight");
_Static_assert(GHOSTTY_KEY_COMMA == 5, "Comma");
_Static_assert(GHOSTTY_KEY_DIGIT_0 == 6, "Digit0");
_Static_assert(GHOSTTY_KEY_DIGIT_1 == 7, "Digit1");
_Static_assert(GHOSTTY_KEY_DIGIT_2 == 8, "Digit2");
_Static_assert(GHOSTTY_KEY_DIGIT_3 == 9, "Digit3");
_Static_assert(GHOSTTY_KEY_DIGIT_4 == 10, "Digit4");
_Static_assert(GHOSTTY_KEY_DIGIT_5 == 11, "Digit5");
_Static_assert(GHOSTTY_KEY_DIGIT_6 == 12, "Digit6");
_Static_assert(GHOSTTY_KEY_DIGIT_7 == 13, "Digit7");
_Static_assert(GHOSTTY_KEY_DIGIT_8 == 14, "Digit8");
_Static_assert(GHOSTTY_KEY_DIGIT_9 == 15, "Digit9");
_Static_assert(GHOSTTY_KEY_EQUAL == 16, "Equal");
_Static_assert(GHOSTTY_KEY_INTL_BACKSLASH == 17, "IntlBackslash");
_Static_assert(GHOSTTY_KEY_INTL_RO == 18, "IntlRo");
_Static_assert(GHOSTTY_KEY_INTL_YEN == 19, "IntlYen");
_Static_assert(GHOSTTY_KEY_A == 20, "KeyA");
_Static_assert(GHOSTTY_KEY_B == 21, "KeyB");
_Static_assert(GHOSTTY_KEY_C == 22, "KeyC");
_Static_assert(GHOSTTY_KEY_D == 23, "KeyD");
_Static_assert(GHOSTTY_KEY_E == 24, "KeyE");
_Static_assert(GHOSTTY_KEY_F == 25, "KeyF");
_Static_assert(GHOSTTY_KEY_G == 26, "KeyG");
_Static_assert(GHOSTTY_KEY_H == 27, "KeyH");
_Static_assert(GHOSTTY_KEY_I == 28, "KeyI");
_Static_assert(GHOSTTY_KEY_J == 29, "KeyJ");
_Static_assert(GHOSTTY_KEY_K == 30, "KeyK");
_Static_assert(GHOSTTY_KEY_L == 31, "KeyL");
_Static_assert(GHOSTTY_KEY_M == 32, "KeyM");
_Static_assert(GHOSTTY_KEY_N == 33, "KeyN");
_Static_assert(GHOSTTY_KEY_O == 34, "KeyO");
_Static_assert(GHOSTTY_KEY_P == 35, "KeyP");
_Static_assert(GHOSTTY_KEY_Q == 36, "KeyQ");
_Static_assert(GHOSTTY_KEY_R == 37, "KeyR");
_Static_assert(GHOSTTY_KEY_S == 38, "KeyS");
_Static_assert(GHOSTTY_KEY_T == 39, "KeyT");
_Static_assert(GHOSTTY_KEY_U == 40, "KeyU");
_Static_assert(GHOSTTY_KEY_V == 41, "KeyV");
_Static_assert(GHOSTTY_KEY_W == 42, "KeyW");
_Static_assert(GHOSTTY_KEY_X == 43, "KeyX");
_Static_assert(GHOSTTY_KEY_Y == 44, "KeyY");
_Static_assert(GHOSTTY_KEY_Z == 45, "KeyZ");
_Static_assert(GHOSTTY_KEY_MINUS == 46, "Minus");
_Static_assert(GHOSTTY_KEY_PERIOD == 47, "Period");
_Static_assert(GHOSTTY_KEY_QUOTE == 48, "Quote");
_Static_assert(GHOSTTY_KEY_SEMICOLON == 49, "Semicolon");
_Static_assert(GHOSTTY_KEY_SLASH == 50, "Slash");
_Static_assert(GHOSTTY_KEY_ALT_LEFT == 51, "AltLeft");
_Static_assert(GHOSTTY_KEY_ALT_RIGHT == 52, "AltRight");
_Static_assert(GHOSTTY_KEY_BACKSPACE == 53, "Backspace");
_Static_assert(GHOSTTY_KEY_CAPS_LOCK == 54, "CapsLock");
_Static_assert(GHOSTTY_KEY_CONTEXT_MENU == 55, "ContextMenu");
_Static_assert(GHOSTTY_KEY_CONTROL_LEFT == 56, "ControlLeft");
_Static_assert(GHOSTTY_KEY_CONTROL_RIGHT == 57, "ControlRight");
_Static_assert(GHOSTTY_KEY_ENTER == 58, "Enter");
_Static_assert(GHOSTTY_KEY_META_LEFT == 59, "MetaLeft");
_Static_assert(GHOSTTY_KEY_META_RIGHT == 60, "MetaRight");
_Static_assert(GHOSTTY_KEY_SHIFT_LEFT == 61, "ShiftLeft");
_Static_assert(GHOSTTY_KEY_SHIFT_RIGHT == 62, "ShiftRight");
_Static_assert(GHOSTTY_KEY_SPACE == 63, "Space");
_Static_assert(GHOSTTY_KEY_TAB == 64, "Tab");
_Static_assert(GHOSTTY_KEY_CONVERT == 65, "Convert");
_Static_assert(GHOSTTY_KEY_KANA_MODE == 66, "KanaMode");
_Static_assert(GHOSTTY_KEY_NON_CONVERT == 67, "NonConvert");
_Static_assert(GHOSTTY_KEY_DELETE == 68, "Delete");
_Static_assert(GHOSTTY_KEY_END == 69, "End");
_Static_assert(GHOSTTY_KEY_HELP == 70, "Help");
_Static_assert(GHOSTTY_KEY_HOME == 71, "Home");
_Static_assert(GHOSTTY_KEY_INSERT == 72, "Insert");
_Static_assert(GHOSTTY_KEY_PAGE_DOWN == 73, "PageDown");
_Static_assert(GHOSTTY_KEY_PAGE_UP == 74, "PageUp");
_Static_assert(GHOSTTY_KEY_ARROW_DOWN == 75, "ArrowDown");
_Static_assert(GHOSTTY_KEY_ARROW_LEFT == 76, "ArrowLeft");
_Static_assert(GHOSTTY_KEY_ARROW_RIGHT == 77, "ArrowRight");
_Static_assert(GHOSTTY_KEY_ARROW_UP == 78, "ArrowUp");
_Static_assert(GHOSTTY_KEY_NUM_LOCK == 79, "NumLock");
_Static_assert(GHOSTTY_KEY_NUMPAD_0 == 80, "Numpad0");
_Static_assert(GHOSTTY_KEY_NUMPAD_1 == 81, "Numpad1");
_Static_assert(GHOSTTY_KEY_NUMPAD_2 == 82, "Numpad2");
_Static_assert(GHOSTTY_KEY_NUMPAD_3 == 83, "Numpad3");
_Static_assert(GHOSTTY_KEY_NUMPAD_4 == 84, "Numpad4");
_Static_assert(GHOSTTY_KEY_NUMPAD_5 == 85, "Numpad5");
_Static_assert(GHOSTTY_KEY_NUMPAD_6 == 86, "Numpad6");
_Static_assert(GHOSTTY_KEY_NUMPAD_7 == 87, "Numpad7");
_Static_assert(GHOSTTY_KEY_NUMPAD_8 == 88, "Numpad8");
_Static_assert(GHOSTTY_KEY_NUMPAD_9 == 89, "Numpad9");
_Static_assert(GHOSTTY_KEY_NUMPAD_ADD == 90, "NumpadAdd");
_Static_assert(GHOSTTY_KEY_NUMPAD_BACKSPACE == 91, "NumpadBackspace");
_Static_assert(GHOSTTY_KEY_NUMPAD_CLEAR == 92, "NumpadClear");
_Static_assert(GHOSTTY_KEY_NUMPAD_CLEAR_ENTRY == 93, "NumpadClearEntry");
_Static_assert(GHOSTTY_KEY_NUMPAD_COMMA == 94, "NumpadComma");
_Static_assert(GHOSTTY_KEY_NUMPAD_DECIMAL == 95, "NumpadDecimal");
_Static_assert(GHOSTTY_KEY_NUMPAD_DIVIDE == 96, "NumpadDivide");
_Static_assert(GHOSTTY_KEY_NUMPAD_ENTER == 97, "NumpadEnter");
_Static_assert(GHOSTTY_KEY_NUMPAD_EQUAL == 98, "NumpadEqual");
_Static_assert(GHOSTTY_KEY_NUMPAD_MEMORY_ADD == 99, "NumpadMemoryAdd");
_Static_assert(GHOSTTY_KEY_NUMPAD_MEMORY_CLEAR == 100, "NumpadMemoryClear");
_Static_assert(GHOSTTY_KEY_NUMPAD_MEMORY_RECALL == 101, "NumpadMemoryRecall");
_Static_assert(GHOSTTY_KEY_NUMPAD_MEMORY_STORE == 102, "NumpadMemoryStore");
_Static_assert(GHOSTTY_KEY_NUMPAD_MEMORY_SUBTRACT == 103, "NumpadMemorySubtract");
_Static_assert(GHOSTTY_KEY_NUMPAD_MULTIPLY == 104, "NumpadMultiply");
_Static_assert(GHOSTTY_KEY_NUMPAD_PAREN_LEFT == 105, "NumpadParenLeft");
_Static_assert(GHOSTTY_KEY_NUMPAD_PAREN_RIGHT == 106, "NumpadParenRight");
_Static_assert(GHOSTTY_KEY_NUMPAD_SUBTRACT == 107, "NumpadSubtract");
_Static_assert(GHOSTTY_KEY_NUMPAD_SEPARATOR == 108, "NumpadSeparator");
_Static_assert(GHOSTTY_KEY_NUMPAD_UP == 109, "NumpadUp");
_Static_assert(GHOSTTY_KEY_NUMPAD_DOWN == 110, "NumpadDown");
_Static_assert(GHOSTTY_KEY_NUMPAD_RIGHT == 111, "NumpadRight");
_Static_assert(GHOSTTY_KEY_NUMPAD_LEFT == 112, "NumpadLeft");
_Static_assert(GHOSTTY_KEY_NUMPAD_BEGIN == 113, "NumpadBegin");
_Static_assert(GHOSTTY_KEY_NUMPAD_HOME == 114, "NumpadHome");
_Static_assert(GHOSTTY_KEY_NUMPAD_END == 115, "NumpadEnd");
_Static_assert(GHOSTTY_KEY_NUMPAD_INSERT == 116, "NumpadInsert");
_Static_assert(GHOSTTY_KEY_NUMPAD_DELETE == 117, "NumpadDelete");
_Static_assert(GHOSTTY_KEY_NUMPAD_PAGE_UP == 118, "NumpadPageUp");
_Static_assert(GHOSTTY_KEY_NUMPAD_PAGE_DOWN == 119, "NumpadPageDown");
_Static_assert(GHOSTTY_KEY_ESCAPE == 120, "Escape");
_Static_assert(GHOSTTY_KEY_F1 == 121, "F1");
_Static_assert(GHOSTTY_KEY_F2 == 122, "F2");
_Static_assert(GHOSTTY_KEY_F3 == 123, "F3");
_Static_assert(GHOSTTY_KEY_F4 == 124, "F4");
_Static_assert(GHOSTTY_KEY_F5 == 125, "F5");
_Static_assert(GHOSTTY_KEY_F6 == 126, "F6");
_Static_assert(GHOSTTY_KEY_F7 == 127, "F7");
_Static_assert(GHOSTTY_KEY_F8 == 128, "F8");
_Static_assert(GHOSTTY_KEY_F9 == 129, "F9");
_Static_assert(GHOSTTY_KEY_F10 == 130, "F10");
_Static_assert(GHOSTTY_KEY_F11 == 131, "F11");
_Static_assert(GHOSTTY_KEY_F12 == 132, "F12");
_Static_assert(GHOSTTY_KEY_F13 == 133, "F13");
_Static_assert(GHOSTTY_KEY_F14 == 134, "F14");
_Static_assert(GHOSTTY_KEY_F15 == 135, "F15");
_Static_assert(GHOSTTY_KEY_F16 == 136, "F16");
_Static_assert(GHOSTTY_KEY_F17 == 137, "F17");
_Static_assert(GHOSTTY_KEY_F18 == 138, "F18");
_Static_assert(GHOSTTY_KEY_F19 == 139, "F19");
_Static_assert(GHOSTTY_KEY_F20 == 140, "F20");
_Static_assert(GHOSTTY_KEY_F21 == 141, "F21");
_Static_assert(GHOSTTY_KEY_F22 == 142, "F22");
_Static_assert(GHOSTTY_KEY_F23 == 143, "F23");
_Static_assert(GHOSTTY_KEY_F24 == 144, "F24");
_Static_assert(GHOSTTY_KEY_F25 == 145, "F25");
_Static_assert(GHOSTTY_KEY_FN == 146, "Fn");
_Static_assert(GHOSTTY_KEY_FN_LOCK == 147, "FnLock");
_Static_assert(GHOSTTY_KEY_PRINT_SCREEN == 148, "PrintScreen");
_Static_assert(GHOSTTY_KEY_SCROLL_LOCK == 149, "ScrollLock");
_Static_assert(GHOSTTY_KEY_PAUSE == 150, "Pause");
_Static_assert(GHOSTTY_KEY_BROWSER_BACK == 151, "BrowserBack");
_Static_assert(GHOSTTY_KEY_BROWSER_FAVORITES == 152, "BrowserFavorites");
_Static_assert(GHOSTTY_KEY_BROWSER_FORWARD == 153, "BrowserForward");
_Static_assert(GHOSTTY_KEY_BROWSER_HOME == 154, "BrowserHome");
_Static_assert(GHOSTTY_KEY_BROWSER_REFRESH == 155, "BrowserRefresh");
_Static_assert(GHOSTTY_KEY_BROWSER_SEARCH == 156, "BrowserSearch");
_Static_assert(GHOSTTY_KEY_BROWSER_STOP == 157, "BrowserStop");
_Static_assert(GHOSTTY_KEY_EJECT == 158, "Eject");
_Static_assert(GHOSTTY_KEY_LAUNCH_APP_1 == 159, "LaunchApp1");
_Static_assert(GHOSTTY_KEY_LAUNCH_APP_2 == 160, "LaunchApp2");
_Static_assert(GHOSTTY_KEY_LAUNCH_MAIL == 161, "LaunchMail");
_Static_assert(GHOSTTY_KEY_MEDIA_PLAY_PAUSE == 162, "MediaPlayPause");
_Static_assert(GHOSTTY_KEY_MEDIA_SELECT == 163, "MediaSelect");
_Static_assert(GHOSTTY_KEY_MEDIA_STOP == 164, "MediaStop");
_Static_assert(GHOSTTY_KEY_MEDIA_TRACK_NEXT == 165, "MediaTrackNext");
_Static_assert(GHOSTTY_KEY_MEDIA_TRACK_PREVIOUS == 166, "MediaTrackPrevious");
_Static_assert(GHOSTTY_KEY_POWER == 167, "Power");
_Static_assert(GHOSTTY_KEY_SLEEP == 168, "Sleep");
_Static_assert(GHOSTTY_KEY_AUDIO_VOLUME_DOWN == 169, "AudioVolumeDown");
_Static_assert(GHOSTTY_KEY_AUDIO_VOLUME_MUTE == 170, "AudioVolumeMute");
_Static_assert(GHOSTTY_KEY_AUDIO_VOLUME_UP == 171, "AudioVolumeUp");
_Static_assert(GHOSTTY_KEY_WAKE_UP == 172, "WakeUp");
_Static_assert(GHOSTTY_KEY_COPY == 173, "Copy");
_Static_assert(GHOSTTY_KEY_CUT == 174, "Cut");
_Static_assert(GHOSTTY_KEY_PASTE == 175, "Paste");

static int compare(GhosttyKeyEncoder encoder, GhosttyKeyEvent event, SfVt *vt,
                   uint32_t key, uint32_t action, uint32_t mods, uint32_t consumed,
                   uint32_t scalar, uint8_t composing, const char *text) {
    uint8_t direct[2048], wrapped[2048];
    size_t direct_len = 0, wrapped_len = 0, size = strlen(text);
    int r = ghostty_key_event_set_key(event, (GhosttyKey)key);
    if (!r) r = ghostty_key_event_set_action(event, (GhosttyKeyAction)action);
    if (!r) r = ghostty_key_event_set_mods(event, (GhosttyMods)mods);
    if (!r) r = ghostty_key_event_set_consumed_mods(event, (GhosttyMods)consumed);
    if (!r) r = ghostty_key_event_set_unshifted_codepoint(event, scalar);
    if (!r) r = ghostty_key_event_set_composing(event, composing != 0);
    GhosttyString string = { .ptr = (const uint8_t *)text, .len = size };
    if (!r) r = ghostty_key_event_set_utf8(event, string);
    if (r) return 0;
    int a = ghostty_key_encoder_encode(encoder, event, direct, sizeof(direct), &direct_len);
    int b = sf_key(vt, key, action, mods, consumed, scalar, composing,
                   (const uint8_t *)text, size, wrapped, sizeof(wrapped), &wrapped_len);
    return a == 0 && b == 0 && direct_len == wrapped_len && !memcmp(direct, wrapped, direct_len);
}

#define CHECK(condition) do { if (!(condition)) { failure = __LINE__; goto done; } } while (0)
int sf_input_v2_public_probe(void) {
    GhosttyKeyEvent event = NULL;
    GhosttyKeyEncoder encoder = NULL;
    GhosttyTerminal terminal = NULL;
    SfVt *vt = NULL;
    int failure = 0;
    CHECK(ghostty_key_event_new(NULL, &event) == 0);
    CHECK(ghostty_key_encoder_new(NULL, &encoder) == 0);
    for (uint32_t action = 0; action < 3; action++) {
        CHECK(ghostty_key_event_set_action(event, (GhosttyKeyAction)action) == 0);
        CHECK((uint32_t)ghostty_key_event_get_action(event) == action);
        for (uint32_t key = 0; key < 176; key++) {
            CHECK(ghostty_key_event_set_key(event, (GhosttyKey)key) == 0);
            CHECK((uint32_t)ghostty_key_event_get_key(event) == key);
        }
        for (uint32_t bits = 0; bits < 1024; bits++) {
            CHECK(ghostty_key_event_set_mods(event, (GhosttyMods)bits) == 0);
            CHECK(ghostty_key_event_get_mods(event) == bits);
            CHECK(ghostty_key_event_set_consumed_mods(event, (GhosttyMods)(bits ^ 1023)) == 0);
            CHECK(ghostty_key_event_get_consumed_mods(event) == (bits ^ 1023));
        }
    }
    const uint32_t valid_scalars[] = { 0, 0x61, 0x1f600, 0x10ffff };
    for (size_t i = 0; i < sizeof(valid_scalars) / sizeof(valid_scalars[0]); i++) {
        CHECK(ghostty_key_event_set_unshifted_codepoint(event, valid_scalars[i]) == 0);
        CHECK(ghostty_key_event_get_unshifted_codepoint(event) == valid_scalars[i]);
    }
    for (unsigned composing = 0; composing < 2; composing++) {
        CHECK(ghostty_key_event_set_composing(event, composing != 0) == 0);
        CHECK(ghostty_key_event_get_composing(event) == (composing != 0));
    }
    const uint8_t borrowed[] = { 0xc2, 0x85, 0xf0, 0x9f, 0x98, 0x80 };
    GhosttyString input = { .ptr = borrowed, .len = sizeof(borrowed) };
    CHECK(ghostty_key_event_set_utf8(event, input) == 0);
    GhosttyString output_text = ghostty_key_event_get_utf8(event);
    CHECK(output_text.ptr == borrowed && output_text.len == sizeof(borrowed));
    CHECK(ghostty_key_event_set_utf8(event, (GhosttyString){ .ptr = NULL, .len = 0 }) == 0);
    CHECK(ghostty_key_event_get_utf8(event).len == 0);
    CHECK(ghostty_key_event_set_key(event, (GhosttyKey)176) == GHOSTTY_INVALID_VALUE);
    CHECK(ghostty_key_event_set_action(event, (GhosttyKeyAction)3) == GHOSTTY_INVALID_VALUE);
    const char *modes[] = { "", "\033[?1h", "\033[?66h", "\033[?66h\033[?1035h", "\033[>4;2m", "\033[>31u" };
    const uint32_t mods[] = { 0, GHOSTTY_MODS_CTRL, GHOSTTY_MODS_SHIFT | GHOSTTY_MODS_CAPS_LOCK, 1023 };
    for (size_t mode = 0; mode < sizeof(modes) / sizeof(modes[0]); mode++) {
        CHECK(ghostty_terminal_new(NULL, &terminal, 80, 24) == 0);
        CHECK(sf_new(80, 24, &vt) == 0);
        ghostty_terminal_vt_write(terminal, (const uint8_t *)modes[mode], strlen(modes[mode]));
        CHECK(sf_feed(vt, (const uint8_t *)modes[mode], strlen(modes[mode])) == 0);
        CHECK(ghostty_key_encoder_set_from_terminal(encoder, terminal) == 0);
        for (uint32_t key = 0; key < 176; key++)
            for (uint32_t action = 0; action < 3; action++)
                for (size_t m = 0; m < sizeof(mods) / sizeof(mods[0]); m++) {
                    CHECK(compare(encoder, event, vt, key, action, mods[m], 0, 0, 0, ""));
                    CHECK(compare(encoder, event, vt, key, action, mods[m], GHOSTTY_MODS_SHIFT, 'e', 0, "\303\251\360\237\230\200"));
                    CHECK(compare(encoder, event, vt, key, action, mods[m], 0, 'a', 1, "a"));
                }
        for (uint32_t bits = 0; bits < 1024; bits++)
            for (uint32_t action = 0; action < 3; action++)
                CHECK(compare(encoder, event, vt, GHOSTTY_KEY_A, action, bits, bits ^ 1023, 'a', 0, "a"));
        size_t n = 99;
        uint8_t output[256];
        for (unsigned bit = 10; bit < 32; bit++) {
            uint32_t bad = UINT32_C(1) << bit;
            CHECK(sf_key(vt, 20, 1, bad, 0, 0, 0, NULL, 0, output, sizeof(output), &n) == GHOSTTY_INVALID_VALUE);
            CHECK(sf_key(vt, 20, 1, 0, bad, 0, 0, NULL, 0, output, sizeof(output), &n) == GHOSTTY_INVALID_VALUE);
        }
        const uint32_t scalars[] = { 0xd800, 0xdfff, 0x110000, 0x200061, UINT32_MAX };
        for (size_t s = 0; s < sizeof(scalars) / sizeof(scalars[0]); s++)
            CHECK(sf_key(vt, 20, 1, 0, 0, scalars[s], 0, NULL, 0, output, sizeof(output), &n) == GHOSTTY_INVALID_VALUE);
        CHECK(sf_key(vt, 176, 1, 0, 0, 0, 0, NULL, 0, output, sizeof(output), &n) == GHOSTTY_INVALID_VALUE);
        CHECK(sf_key(vt, 20, 3, 0, 0, 0, 0, NULL, 0, output, sizeof(output), &n) == GHOSTTY_INVALID_VALUE);
        CHECK(sf_key(vt, 20, 1, 0, 0, 0, 2, NULL, 0, output, sizeof(output), &n) == GHOSTTY_INVALID_VALUE);
        CHECK(sf_key(vt, 20, 1, 0, 0, 0, 0, NULL, 1, output, sizeof(output), &n) == GHOSTTY_INVALID_VALUE);
        CHECK(sf_key(vt, 20, 1, 0, 0, 0, 0, NULL, 0, NULL, 1, &n) == GHOSTTY_INVALID_VALUE);
        CHECK(sf_key(vt, 20, 1, 0, 0, 0, 0, NULL, 0, output, sizeof(output), NULL) == GHOSTTY_INVALID_VALUE);
        sf_free(vt); vt = NULL;
        ghostty_terminal_free(terminal); terminal = NULL;
    }
done:
    sf_free(vt);
    ghostty_terminal_free(terminal);
    ghostty_key_encoder_free(encoder);
    ghostty_key_event_free(event);
    return failure;
}
