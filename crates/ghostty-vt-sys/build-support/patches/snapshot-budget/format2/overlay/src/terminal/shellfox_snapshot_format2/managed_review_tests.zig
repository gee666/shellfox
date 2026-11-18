const std = @import("std");
const testing = std.testing;
const api = @import("../c/shellfox_snapshot_format2.zig");
const ct = @import("../c/terminal.zig");
const fixtures = @import("codec_tests.zig");
const crc = @import("../../crc32c.zig").Crc32c;
const Result = @import("../c/result.zig").Result;
fn find(bytes: []const u8, tag: u16) !usize {
    var pos: usize = 32;
    while (pos < bytes.len) {
        if (std.mem.readInt(u16, bytes[pos..][0..2], .little) == tag) return pos;
        pos += 10 + std.mem.readInt(u32, bytes[pos + 2 ..][0..4], .little);
    }
    return error.MissingRecord;
}
fn repair(bytes: []u8, pos: usize) void {
    const n = std.mem.readInt(u32, bytes[pos + 2 ..][0..4], .little);
    var h = crc.init();
    h.update(bytes[pos..][0..6]);
    h.update(bytes[pos + 10 ..][0..n]);
    std.mem.writeInt(u32, bytes[pos + 6 ..][0..4], h.final(), .little);
}
test "format2 review whole blob concrete aliases refuse before native allocation" {
    const t = try fixtures.source();
    defer ct.free(t);
    ct.vt_write(t, "A", 1);
    const o = fixtures.options();
    var source: [65536]u8 = undefined;
    var length: usize = 0;
    try testing.expectEqual(Result.success, api.encode_buf(t, &o, &source, source.len, &length, null));
    const at = try find(source[0..length], 0x2005);
    const old_length = std.mem.readInt(u32, source[at + 2 ..][0..4], .little);
    inline for (.{ false, true }) |link| {
        var output: [65536]u8 = undefined;
        @memcpy(output[0 .. at + 10], source[0 .. at + 10]);
        var writer: std.Io.Writer = .fixed(output[at + 10 ..]);
        const io = @import("../snapshot/io.zig");
        try io.writeInt(&writer, u16, if (link) 0 else 128);
        try io.writeInt(&writer, u16, if (link) 128 else 0);
        for (1..129) |id| {
            try io.writeInt(&writer, u16, @intCast(id));
            if (link) try @import("../snapshot/hyperlink.zig").encode(.{ .id = .{ .implicit = 7 }, .uri = "alias" }, &writer) else try @import("../snapshot/style.zig").encode(.{ .flags = .{ .bold = true } }, &writer);
        }
        try writer.writeAll(source[at + 14 ..][0 .. old_length - 4]);
        const next = at + 10 + writer.end;
        const tail = source[at + 10 + old_length .. length];
        @memcpy(output[next..][0..tail.len], tail);
        const len = next + tail.len;
        std.mem.writeInt(u32, output[at + 2 ..][0..4], @intCast(writer.end), .little);
        std.mem.writeInt(u64, output[24..32], len, .little);
        repair(output[0..len], at);
        var result: ct.Terminal = null;
        var report: api.Report = .{};
        try testing.expectEqual(Result.invalid_value, api.decode_buf(&output, len, &o, &result, &report));
        try testing.expect(result == null);
        try testing.expectEqual(@as(u64, 0), report.heap_bytes + report.mapped_bytes);
    }
}

test "format2 review managed cleanup covers tables graphemes cursor links and both screens" {
    const t = try fixtures.source();
    defer ct.free(t);
    const prefix = "\x1b[31m\x1b]8;id=p;https://example.invalid/p\x1b\\A\xcc\x81\xcc\x80\x1b7" ++
        "\x1b[?1049h\x1b[44m\x1b]8;id=a;https://example.invalid/a\x1b\\BC\xcc\x81\x1b7";
    ct.vt_write(t, prefix, prefix.len);
    var bytes: [65536]u8 = undefined;
    var len: usize = 0;
    const o = fixtures.options();
    try testing.expectEqual(Result.success, api.encode_buf(t, &o, &bytes, bytes.len, &len, null));
    var result: ct.Terminal = null;
    var report: api.Report = .{};
    try testing.expectEqual(Result.success, api.decode_buf(&bytes, len, &o, &result, &report));
    try @import("compare_tests.zig").terminals(t, result);
    ct.free(result);
    const total = report.heap_bytes + report.mapped_bytes;
    var budget: u64 = 0;
    while (budget < total) : (budget += 65536) {
        var small = o;
        small.max_allocation_bytes = budget;
        result = null;
        try testing.expectEqual(Result.limit_exceeded, api.decode_buf(&bytes, len, &small, &result, &report));
        try testing.expect(result == null);
    }
    // Force a late managed-table failure after terminal and screen allocation.
    const at = try find(bytes[0..len], 0x2004);
    var changed = bytes;
    // Page-layout fixed fields: logical/capacity dimensions, style/link hints,
    // then grapheme bytes at14. This preserves framing and payload semantics.
    std.mem.writeInt(u32, changed[at + 10 + 14 ..][0..4], 0, .little);
    repair(changed[0..len], at);
    result = null;
    try testing.expect(api.decode_buf(&changed, len, &o, &result, &report) != .success);
    try testing.expect(result == null);
}

test "format2 review capacity dependent work refuses high sparse capacity before alloc" {
    const t = try fixtures.source();
    defer ct.free(t);
    const prefix = "A\xcc\x81\xcc\x80";
    ct.vt_write(t, prefix, prefix.len);
    var bytes: [65536]u8 = undefined;
    var len: usize = 0;
    const o = fixtures.options();
    try testing.expectEqual(Result.success, api.encode_buf(t, &o, &bytes, bytes.len, &len, null));
    const at = try find(bytes[0..len], 0x2004);
    var larger = bytes;
    std.mem.writeInt(u16, larger[at + 10 + 8 ..][0..2], 32768, .little); // style capacity
    std.mem.writeInt(u32, larger[at + 10 + 18 ..][0..4], 64 * 1024 * 1024, .little); // string capacity
    repair(larger[0..len], at);
    var measured: api.Report = .{};
    var result: ct.Terminal = null;
    try testing.expectEqual(Result.success, api.decode_buf(&bytes, len, &o, &result, &measured));
    ct.free(result);
    var low = o;
    low.max_work_units = measured.work_units;
    result = null;
    var report: api.Report = .{};
    try testing.expectEqual(Result.limit_exceeded, api.decode_buf(&larger, len, &low, &result, &report));
    try testing.expect(result == null);
    try testing.expectEqual(@as(u64, 0), report.heap_bytes + report.mapped_bytes);
}
