const std = @import("std");
const testing = std.testing;
const api = @import("../c/shellfox_snapshot_format2.zig");
const ct = @import("../c/terminal.zig");
const fixtures = @import("codec_tests.zig");
const guard = @import("../snapshot_budget_guard.zig");
const profile = @import("../shellfox_terminal_profile.zig");
const Result = @import("../c/result.zig").Result;

fn digest(t: ct.Terminal) u64 {
    var hash = std.hash.Wyhash.init(0);
    const native = ct.zigTerminal(t).?;
    for ([_]@import("../ScreenSet.zig").Key{ .primary, .alternate }) |key| if (native.screens.get(key)) |s| {
        var n = s.pages.pages.first;
        while (n) |node| : (n = node.next) switch (node.data) {
            .compressed => |data| hash.update(data.encoded),
            .resident => |data| hash.update(data.memory),
        };
    };
    return hash.final();
}
test "format2 review resident preflight refuses compressed source without mutation" {
    const t = try fixtures.source();
    defer ct.free(t);
    const control = try fixtures.source();
    defer ct.free(control);
    const text = "abcdefgh\r\n";
    for (0..12000) |_| {
        ct.vt_write(t, text, text.len);
        ct.vt_write(control, text, text.len);
    }
    const native = ct.zigTerminal(t).?;
    if (native.compress(.full) == .unsupported) return error.SkipZigTest;
    for (0..64) |_| {
        if (native.compress(.full) == .complete) break;
    }
    const before = native.screens.get(.primary).?.pages.memoryUsage();
    try testing.expect(before.compressed_pages > 0);
    const state = try profile.capture(native);
    const data = digest(t);
    var buffer: [65536]u8 = @splat(0xa5);
    var written: usize = 99;
    const o = fixtures.options();
    try testing.expectEqual(Result.unsupported, api.encode_buf(t, &o, &buffer, buffer.len, &written, null));
    try testing.expectEqual(@as(usize, 0), written);
    for (buffer) |byte| try testing.expectEqual(@as(u8, 0xa5), byte);
    try testing.expectEqual(before, native.screens.get(.primary).?.pages.memoryUsage());
    try testing.expectEqual(data, digest(t));
    try testing.expectEqual(state, try profile.capture(native));
    // Only after proving refusal unchanged may a deliberate control comparison
    // rehydrate pages. The same future suffix must still behave identically.
    const suffix = "\x1b[2;1HZ\r\n";
    ct.vt_write(t, suffix, suffix.len);
    ct.vt_write(control, suffix, suffix.len);
    var pages = native.screens.get(.primary).?.pages.pages.first;
    while (pages) |node| : (pages = node.next) _ = node.page();
    try @import("compare_tests.zig").terminals(t, control);
}

test "format2 review distinct concrete table aliases reject before native refs" {
    const tables = @import("tables.zig");
    const wire = @import("../snapshot/io.zig");
    const style = @import("../snapshot/style.zig");
    const links = @import("../snapshot/hyperlink.zig");
    inline for (.{ false, true }) |link_alias| {
        var scope: guard.Scope = .{ .max_bytes = 16 * 1024 * 1024, .max_work = 1000000000000000 };
        try testing.expect(guard.enter(&scope));
        defer guard.leave();
        var target = try @import("../page.zig").Page.init(.{ .cols = 8, .rows = 3, .styles = 128, .hyperlink_bytes = 1024, .hyperlink_cells = 64, .string_bytes = 2048 });
        defer target.deinit();
        var buffer: [16384]u8 = undefined;
        var writer: std.Io.Writer = .fixed(&buffer);
        try wire.writeInt(&writer, u16, if (link_alias) 0 else 256);
        try wire.writeInt(&writer, u16, if (link_alias) 256 else 0);
        for (1..257) |id| {
            try wire.writeInt(&writer, u16, @intCast(id));
            if (link_alias) try links.encode(.{ .id = .{ .implicit = 7 }, .uri = "same" }, &writer) else try style.encode(.{ .flags = .{ .bold = true } }, &writer);
        }
        const before_heap = scope.heap_bytes;
        var r: std.Io.Reader = .fixed(writer.buffered());
        if (link_alias) try testing.expectError(error.NoncanonicalHyperlinkAlias, tables.decode(&r, &target, null, target.memory.len)) else try testing.expectError(error.NoncanonicalStyleAlias, tables.decode(&r, &target, null, target.memory.len));
        try testing.expectEqual(before_heap, scope.heap_bytes);
        try testing.expectEqual(@as(usize, 0), target.styles.count());
        try testing.expectEqual(@as(usize, 0), target.hyperlink_set.count());
    }
}

test "format2 review writer errors distinguish capacity encoded and continuation limits" {
    const t = try fixtures.source();
    defer ct.free(t);
    const prefix = "\x1b]2;unfinished";
    ct.vt_write(t, prefix, prefix.len);
    const o = fixtures.options();
    var full: [65536]u8 = undefined;
    var length: usize = 0;
    try testing.expectEqual(Result.success, api.encode_buf(t, &o, &full, full.len, &length, null));
    var output: [65536]u8 = @splat(0xa5);
    for ([_]usize{ 33, 80, length / 2, length - 1 }) |cap| {
        var written: usize = 99;
        try testing.expectEqual(Result.out_of_space, api.encode_buf(t, &o, &output, cap, &written, null));
        try testing.expectEqual(@as(usize, 0), written);
        for (output) |byte| try testing.expectEqual(@as(u8, 0xa5), byte);
    }
    inline for (.{ "max_encoded_bytes", "max_continuation_bytes" }) |field| {
        var reduced = o;
        @field(reduced, field) = if (std.mem.eql(u8, field, "max_encoded_bytes")) length - 1 else 1;
        var written: usize = 99;
        try testing.expectEqual(Result.limit_exceeded, api.encode_buf(t, &reduced, &output, output.len, &written, null));
        try testing.expectEqual(@as(usize, 0), written);
        for (output) |byte| try testing.expectEqual(@as(u8, 0xa5), byte);
    }
    inline for (.{ false, true }) |continuation_limit| {
        var reduced = o;
        if (continuation_limit) reduced.max_continuation_bytes = 1 else reduced.max_encoded_bytes = length - 1;
        var scope: guard.Scope = .{ .max_bytes = reduced.max_allocation_bytes, .max_work = reduced.max_work_units };
        try testing.expect(guard.enter(&scope));
        defer guard.leave();
        if (continuation_limit) try testing.expectError(error.ContinuationLimitExceeded, @import("codec.zig").encode(t, reduced, &scope, output.len)) else try testing.expectError(error.EncodedLimitExceeded, @import("codec.zig").encode(t, reduced, &scope, output.len));
    }
    var exact: usize = 0;
    try testing.expectEqual(Result.success, api.encode_buf(t, &o, &output, length, &exact, null));
    try testing.expectEqual(length, exact);
}
