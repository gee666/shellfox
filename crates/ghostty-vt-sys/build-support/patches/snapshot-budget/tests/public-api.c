/* Public ABI smoke test. No private Ghostty headers. MIT, see LICENSE. */
#include <ghostty/vt/shellfox_snapshot_budget.h>
#include <ghostty/vt/formatter.h>
#include <ghostty/vt/shellfox_extension.h>
#include <stdbool.h>
#include <stdio.h>
#include <string.h>

#define CHECK(expr) do { if (!(expr)) { \
  fprintf(stderr, "check failed at line %d: %s\n", __LINE__, #expr); \
  return 1; } } while (0)

int main(int argc, char **argv) {
  CHECK(argc == 2); /* Exact externally supplied build-profile identity. */
  GhosttyString identity = { .ptr = (const uint8_t *)argv[1], .len = strlen(argv[1]) };
  GhosttyShellfoxExtensionInfo info = { .size = sizeof(info), .abi_version = 1 };
  CHECK(ghostty_shellfox_extension_info(&info) == GHOSTTY_SUCCESS);
  CHECK(info.snapshot_budget_abi_version == 1 && info.terminal_profile_abi_version == 1);
  CHECK((info.features & GHOSTTY_SHELLFOX_FEATURE_SNAPSHOT_BUDGET) != 0);
  CHECK((info.features & GHOSTTY_SHELLFOX_FEATURE_TERMINAL_PROFILE) != 0);
  CHECK(info.engine_profile.len == identity.len &&
        memcmp(info.engine_profile.ptr, identity.ptr, identity.len) == 0);
  GhosttySnapshotBudgetOptions options = {
    .size = sizeof(options), .abi_version = 1, .profile = 1,
    .ghostty_version = GHOSTTY_SNAPSHOT_BUDGET_GHOSTTY_1_0_1,
    .snapshot_version = 1, .engine_profile = identity,
    .profile_state = { .size = sizeof(GhosttyShellfoxTerminalProfileState),
      .abi_version = 1, .terminal_profile = 0, .preceding_join = 0 },
    .max_encoded_bytes = 4 * 1024 * 1024,
    .max_allocation_bytes = 128 * 1024 * 1024,
    .max_work_units = UINT64_C(100000000000), .max_cells = 1000000,
    .max_history_rows = 10000, .max_continuation_bytes = 4096,
    .runtime_image_storage_limit = 10000000, /* Exact source configuration. */
    .runtime_image_io_policy = GHOSTTY_SNAPSHOT_BUDGET_IMAGE_IO_DIRECT_ONLY,
    .runtime_resize_pull_scrollback = GHOSTTY_SNAPSHOT_BUDGET_PULL_SCROLLBACK_ENABLED,
    .max_columns = 1000, .max_rows = 1000,
    .max_pixel_width = 4096, .max_pixel_height = 4096,
    .max_page_rows = 65535, .max_pages = 1000, .max_records = 10000,
  };
  GhosttySnapshotBudgetEncodeOptions encode = {
    .size = sizeof(encode), .abi_version = 1, .profile = 1,
    .ghostty_version = GHOSTTY_SNAPSHOT_BUDGET_GHOSTTY_1_0_1,
    .snapshot_version = 1, .engine_profile = identity, .terminal_profile = 0,
    .max_encoded_bytes = options.max_encoded_bytes,
    .max_allocation_bytes = options.max_allocation_bytes,
    .max_work_units = options.max_work_units, .max_cells = options.max_cells,
    .max_history_rows = options.max_history_rows,
    .max_continuation_bytes = options.max_continuation_bytes,
    .runtime_image_storage_limit = options.runtime_image_storage_limit,
    .runtime_image_io_policy = options.runtime_image_io_policy,
    .runtime_resize_pull_scrollback = options.runtime_resize_pull_scrollback,
    .max_columns = options.max_columns, .max_rows = options.max_rows,
    .max_pixel_width = options.max_pixel_width, .max_pixel_height = options.max_pixel_height,
    .max_page_rows = options.max_page_rows, .max_pages = options.max_pages,
    .max_records = options.max_records,
  };
  GhosttySnapshotBudgetReport report = { .size = sizeof(report), .abi_version = 1 };
  GhosttyShellfoxTerminalProfileOptions constructor = {
    .size = sizeof(constructor), .abi_version = 1, .engine_profile = identity,
    .terminal_profile = GHOSTTY_SHELLFOX_TERMINAL_PROFILE_NATIVE, .cols = 8, .rows = 3,
  };
  GhosttyTerminal source = NULL;
  GhosttyTerminal restored = NULL;
  CHECK(ghostty_shellfox_terminal_new_profile(NULL, &source, &constructor) == GHOSTTY_SUCCESS);
  size_t tail_limit = 4096;
  CHECK(ghostty_terminal_set(source, GHOSTTY_TERMINAL_OPT_CONTINUATION_MAX_BYTES,
        &tail_limit) == GHOSTTY_SUCCESS);
  const char *input = "hello\r\nworld\033[?1049halt\033[?1049l\033[38;2;";
  ghostty_terminal_vt_write(source, (const uint8_t *)input, strlen(input));
  uint8_t bytes[65536];
  memset(bytes, 0xa5, sizeof(bytes));
  size_t length = 99;
  GhosttyShellfoxTerminalProfileState cut = {
    .size = sizeof(cut), .abi_version = 1, .terminal_profile = 0, .preceding_join = 0,
  };
  CHECK(ghostty_shellfox_snapshot_budget_encode_buf(source, &encode, bytes, 1,
        &length, &cut, &report) == GHOSTTY_OUT_OF_SPACE);
  CHECK(length == 0 && cut.size == 0 && cut.abi_version == 0);
  for (size_t i = 0; i < sizeof(bytes); ++i) CHECK(bytes[i] == 0xa5);
  cut.size = sizeof(cut); cut.abi_version = 1;
  CHECK(ghostty_shellfox_snapshot_budget_encode_buf(source, &encode, bytes, sizeof(bytes),
        &length, &cut, &report) == GHOSTTY_SUCCESS);
  CHECK(cut.terminal_profile == 0 && cut.preceding_join == 0);
  options.profile_state = cut;
  CHECK(ghostty_shellfox_snapshot_budget_decode_buf(bytes, length, &options,
        &restored, &report) == GHOSTTY_SUCCESS);
  CHECK(restored != NULL && report.source_offset == length);
  CHECK(report.heap_bytes > 0 && report.mapped_bytes > 0);
  CHECK(report.heap_bytes + report.mapped_bytes <= options.max_allocation_bytes);
  CHECK(report.work_units <= options.max_work_units);
  const char *suffix = "10;20;30m!";
  ghostty_terminal_vt_write(source, (const uint8_t *)suffix, strlen(suffix));
  ghostty_terminal_vt_write(restored, (const uint8_t *)suffix, strlen(suffix));
  uint16_t source_x = 0, source_y = 0, restored_x = 0, restored_y = 0;
  CHECK(ghostty_terminal_get(source, GHOSTTY_TERMINAL_DATA_CURSOR_X, &source_x) == GHOSTTY_SUCCESS);
  CHECK(ghostty_terminal_get(source, GHOSTTY_TERMINAL_DATA_CURSOR_Y, &source_y) == GHOSTTY_SUCCESS);
  CHECK(ghostty_terminal_get(restored, GHOSTTY_TERMINAL_DATA_CURSOR_X, &restored_x) == GHOSTTY_SUCCESS);
  CHECK(ghostty_terminal_get(restored, GHOSTTY_TERMINAL_DATA_CURSOR_Y, &restored_y) == GHOSTTY_SUCCESS);
  CHECK(source_x == restored_x && source_y == restored_y);
  GhosttyFormatter source_formatter = NULL, restored_formatter = NULL;
  CHECK(ghostty_formatter_terminal_new(NULL, &source_formatter, source) == GHOSTTY_SUCCESS);
  CHECK(ghostty_formatter_terminal_new(NULL, &restored_formatter, restored) == GHOSTTY_SUCCESS);
  uint8_t source_text[4096], restored_text[4096];
  size_t source_len = 0, restored_len = 0;
  CHECK(ghostty_formatter_format_buf(source_formatter, source_text, sizeof(source_text), &source_len) == GHOSTTY_SUCCESS);
  CHECK(ghostty_formatter_format_buf(restored_formatter, restored_text, sizeof(restored_text), &restored_len) == GHOSTTY_SUCCESS);
  CHECK(source_len == restored_len && memcmp(source_text, restored_text, source_len) == 0);
  ghostty_formatter_free(source_formatter); ghostty_formatter_free(restored_formatter);
  GhosttyShellfoxTerminalProfileState actual = { .size = sizeof(actual), .abi_version = 1 };
  CHECK(ghostty_shellfox_terminal_get_profile_state(restored, &actual) == GHOSTTY_SUCCESS);
  CHECK(actual.terminal_profile == cut.terminal_profile && actual.preceding_join == cut.preceding_join);
  ghostty_terminal_free(restored);
  restored = source; /* Failure must clear a preexisting non-null output. */
  options.max_allocation_bytes = 0;
  CHECK(ghostty_shellfox_snapshot_budget_decode_buf(bytes, length, &options,
        &restored, &report) == GHOSTTY_LIMIT_EXCEEDED);
  CHECK(restored == NULL);
  options.max_allocation_bytes = 128 * 1024 * 1024;
  options.profile_state.terminal_profile = GHOSTTY_SHELLFOX_TERMINAL_PROFILE_LEGACY_XTERM6;
  options.profile_state.preceding_join = 2;
  CHECK(ghostty_shellfox_snapshot_budget_decode_buf(bytes, length, &options,
        &restored, NULL) == GHOSTTY_UNSUPPORTED);
  options.profile_state = cut; options.flags = 1;
  CHECK(ghostty_shellfox_snapshot_budget_decode_buf(bytes, length, &options,
        &restored, NULL) == GHOSTTY_UNSUPPORTED);
  options.flags = 0; options.size--;
  CHECK(ghostty_shellfox_snapshot_budget_decode_buf(bytes, length, &options,
        &restored, NULL) == GHOSTTY_INVALID_VALUE);
  options.size++;
  CHECK(ghostty_shellfox_snapshot_budget_decode_buf(bytes, length - 1, &options,
        &restored, NULL) == GHOSTTY_INVALID_VALUE);
  CHECK(restored == NULL);
  ghostty_terminal_free(source);
  printf("public Native snapshot State/body ABI passed; decode=%zu encode=%zu State=%zu report=%zu; resize offsets=%zu/%zu\n",
         sizeof(options), sizeof(encode), sizeof(cut), sizeof(report),
         offsetof(GhosttySnapshotBudgetOptions, runtime_resize_pull_scrollback),
         offsetof(GhosttySnapshotBudgetEncodeOptions, runtime_resize_pull_scrollback));
  return 0;
}
