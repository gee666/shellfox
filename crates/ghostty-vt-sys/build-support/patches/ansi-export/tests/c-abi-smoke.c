/* Shellfox additive extension ABI smoke test. MIT. No PTY or user data. */
#include <ghostty/vt.h>
#include <ghostty/vt/shellfox_extension.h>
#include <ghostty/vt/shellfox_ansi.h>
#include <stdlib.h>
#include <stdio.h>
#include <string.h>
#define CHECK(call) do { GhosttyResult result_ = (call); if (result_ != GHOSTTY_SUCCESS) { fprintf(stderr, "%s: %d\n", #call, (int)result_); return 1; } } while (0)
int main(void) {
    GhosttyShellfoxExtensionInfo info = {0};
    info.size = sizeof(info); info.abi_version = GHOSTTY_SHELLFOX_EXTENSION_ABI;
    CHECK(ghostty_shellfox_extension_info(&info));
    if (info.ansi_abi_version != 1 || !(info.features & GHOSTTY_SHELLFOX_FEATURE_ANSI_FULL) || info.engine_profile.len != 64) return 2;
    GhosttyTerminal terminal = NULL;
    CHECK(ghostty_terminal_new(NULL, &terminal, 20, 5));
    GhosttyCursorStyle style = GHOSTTY_CURSOR_STYLE_BLOCK;
    bool blinking = false;
    CHECK(ghostty_terminal_set(terminal, GHOSTTY_TERMINAL_OPT_DEFAULT_CURSOR_STYLE, &style));
    CHECK(ghostty_terminal_set(terminal, GHOSTTY_TERMINAL_OPT_DEFAULT_CURSOR_BLINK, &blinking));
    const char *fixture = "fixture\x1b[2;2H\x1b" "7\x1b[3;4Hz";
    ghostty_terminal_vt_write(terminal, (const uint8_t*)fixture, strlen(fixture));
    GhosttyShellfoxAnsiOptions options = {0};
    options.size = sizeof(options); options.abi_version = GHOSTTY_SHELLFOX_ANSI_ABI;
    options.target = GHOSTTY_SHELLFOX_TARGET_NATIVE_VT1; options.target_features = GHOSTTY_SHELLFOX_TARGET_KITTY_KEYBOARD;
    options.engine_profile = info.engine_profile;
    options.max_output_bytes = 1024 * 1024; options.max_rows = 1000; options.max_cells = 20000;
    options.max_continuation_bytes = 65536; options.default_cursor_style = (uint32_t)style;
    options.default_cursor_blink = 0; options.runtime_resize_pull_scrollback = 1; options.target_is_fresh = 1;
    GhosttyResult bytes_limit = ghostty_terminal_get(terminal, GHOSTTY_TERMINAL_DATA_SCROLLBACK_MAX_BYTES, &options.target_scrollback_max_bytes);
    if (bytes_limit == GHOSTTY_NO_VALUE) options.target_scrollback_max_bytes = SIZE_MAX;
    else if (bytes_limit != GHOSTTY_SUCCESS) return 6;
    GhosttyResult rows_limit = ghostty_terminal_get(terminal, GHOSTTY_TERMINAL_DATA_SCROLLBACK_MAX_LINES, &options.target_scrollback_max_rows);
    if (rows_limit == GHOSTTY_NO_VALUE) options.target_scrollback_max_rows = SIZE_MAX;
    else if (rows_limit != GHOSTTY_SUCCESS) return 7;
    CHECK(ghostty_terminal_get(terminal, GHOSTTY_TERMINAL_DATA_KITTY_IMAGE_STORAGE_LIMIT, &options.runtime_image_storage_limit));
    size_t required = 0; uint64_t unsupported = 0;
    if (ghostty_terminal_export_ansi_full_buf(terminal, &options, NULL, 0, &required, &unsupported) != GHOSTTY_OUT_OF_SPACE || !required || unsupported) return 3;
    uint8_t *bytes = (uint8_t*)malloc(required);
    if (!bytes) return 4;
    CHECK(ghostty_terminal_export_ansi_full_buf(terminal, &options, bytes, required, &required, &unsupported));
    if (unsupported || !required) return 5;
    free(bytes); ghostty_terminal_free(terminal);
    printf("Shellfox extension C ABI smoke PASS: info=%zu options=%zu\n", sizeof(info), sizeof(options));
    return 0;
}
