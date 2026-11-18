const std = @import("std");
const testing = std.testing;
const api = @import("../c/shellfox_snapshot_format2.zig");
const ct = @import("../c/terminal.zig");
const lib = @import("../lib.zig");
const layout = @import("../shellfox_cell_layout2.zig");
const profile = @import("../shellfox_terminal_profile.zig");
const guard = @import("../snapshot_budget_guard.zig");
const raw = @import("../snapshot/main.zig");
const Result = @import("../c/result.zig").Result;

pub fn options() api.Options {
    return .{ .engine_profile = lib.String.init(@as([]const u8, @import("../shellfox_profile.zig").engine_profile)), .runtime_resize_pull_scrollback = 1, .max_encoded_bytes = 4 * 1024 * 1024, .max_allocation_bytes = 256 * 1024 * 1024, .max_work_units = 1000000000000000, .max_physical_slots = 1000000, .max_rows = 100000, .max_text_scalars = 1000000, .max_text_bytes = 4 * 1024 * 1024, .max_continuation_bytes = 4096, .max_pages = 1024, .max_records = 4096, .max_visible_columns = 4096, .max_backing_columns = 4096, .max_visible_rows = 4096, .max_pixel_width = 65535, .max_pixel_height = 65535 };
}
pub fn source() !ct.Terminal {
    return sourceSized(8, 3);
}
pub fn sourceSized(cols: u16, rows: u16) !ct.Terminal {
    var scope: guard.Scope = .{ .max_bytes = 256 * 1024 * 1024, .max_work = 1000000000000 };
    try testing.expect(guard.enter(&scope));
    defer guard.leave();
    var fresh = try layout.newFreshRestore(.{ .cols = cols, .rows = rows, .default_modes = .{ .grapheme_cluster = false }, .kitty_image_storage_limit = 0, .max_scrollback_bytes = 16 * 1024 * 1024 });
    defer fresh.deinit();
    const state: profile.State = .{ .terminal_profile = 1, .preceding_join = 0 };
    try fresh.installProfileState(&state, .{ .max_cells = 1000000, .max_rows = 100000 });
    var decoded: raw.Decoded = .{ .terminal = try fresh.takeForContinuation(), .continuation = .ground, .history_rows = .init(.{}) };
    defer decoded.deinit(guard.heapAllocator());
    return ct.fromDecoded(guard.heapAllocator(), .init, &decoded, 4096);
}
test "format2 generate owned public boundary fixture when requested" {
    const path = testing.environ.getAlloc(testing.allocator, "SHELLFOX_FORMAT2_FIXTURE") catch |err| switch (err) {
        error.EnvironmentVariableMissing => return error.SkipZigTest,
        else => return err,
    };
    defer testing.allocator.free(path);
    const t = try source();
    defer ct.free(t);
    const prefix = "primary\x1b[?1049hABCDEF\x1b[0m\xcc\x81";
    ct.vt_write(t, prefix, prefix.len);
    try testing.expectEqual(Result.success, ct.resize(t, 4, 5, 0, 0));
    const pending = "\x1b[38;2;";
    ct.vt_write(t, pending, pending.len);
    const bytes = try testing.allocator.alloc(u8, 1024 * 1024);
    defer testing.allocator.free(bytes);
    const o = options();
    var len: usize = 0;
    try testing.expectEqual(Result.success, api.encode_buf(t, &o, bytes.ptr, bytes.len, &len, null));
    try std.Io.Dir.cwd().writeFile(testing.io, .{ .sub_path = path, .data = bytes[0..len] });
}

test "format2 codec actual canonical source body restore and suffix" {
    const original = try source();
    defer ct.free(original);
    const prefix = "A\x1b[0m\xcc\x81\r\nhello\x1b[38;2;";
    ct.vt_write(original, prefix, prefix.len);
    var bytes: [65536]u8 = undefined;
    var len: usize = 0;
    const o = options();
    try testing.expectEqual(Result.success, api.encode_buf(original, &o, &bytes, bytes.len, &len, null));
    var restored: ct.Terminal = null;
    try testing.expectEqual(Result.success, api.decode_buf(&bytes, len, &o, &restored, null));
    defer ct.free(restored);
    const suffix = "12;34;56m!";
    ct.vt_write(original, suffix, suffix.len);
    ct.vt_write(restored, suffix, suffix.len);
    const a = ct.zigTerminal(original).?;
    const b = ct.zigTerminal(restored).?;
    const at = try a.plainString(testing.allocator);
    defer testing.allocator.free(at);
    const bt = try b.plainString(testing.allocator);
    defer testing.allocator.free(bt);
    try testing.expectEqualStrings(at, bt);
    const ac = try profile.capture(a);
    const bc = try profile.capture(b);
    try testing.expectEqual(ac.preceding_join, bc.preceding_join);
    try testing.expectEqual(a.screens.active.cursor.x, b.screens.active.cursor.x);
}
