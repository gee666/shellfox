const std = @import("std");
const testing = std.testing;
const api = @import("../c/shellfox_snapshot_format2.zig");
const ct = @import("../c/terminal.zig");
const fixtures = @import("codec_tests.zig");
const crc = @import("../../crc32c.zig").Crc32c;
const Result = @import("../c/result.zig").Result;
fn find(bytes: []const u8, tag: u16) !usize {
    var offset: usize = 32;
    while (offset < bytes.len) {
        if (std.mem.readInt(u16, bytes[offset..][0..2], .little) == tag) return offset;
        offset += 10 + std.mem.readInt(u32, bytes[offset + 2 ..][0..4], .little);
    }
    return error.MissingRecord;
}
fn repair(bytes: []u8, offset: usize) void {
    const size = std.mem.readInt(u32, bytes[offset + 2 ..][0..4], .little);
    var hash = crc.init();
    hash.update(bytes[offset..][0..6]);
    hash.update(bytes[offset + 10 ..][0..size]);
    std.mem.writeInt(u32, bytes[offset + 6 ..][0..4], hash.final(), .little);
}
fn refuse(bytes: []const u8, o: api.Options, expected: Result) !api.Report {
    var output: ct.Terminal = null;
    var report: api.Report = .{};
    try testing.expectEqual(expected, api.decode_buf(bytes.ptr, bytes.len, &o, &output, &report));
    try testing.expect(output == null);
    try testing.expect(report.heap_bytes + report.mapped_bytes <= o.max_allocation_bytes);
    try testing.expect(report.work_units <= o.max_work_units);
    return report;
}
test "format2 codec malformed cells layouts truncations FINISH and budgets are atomic" {
    const source = try fixtures.source();
    defer ct.free(source);
    ct.vt_write(source, "A", 1);
    const o = fixtures.options();
    var original: [65536]u8 = undefined;
    var len: usize = 0;
    try testing.expectEqual(Result.success, api.encode_buf(source, &o, &original, original.len, &len, null));
    for (0..len) |cut| _ = try refuse(original[0..cut], o, .invalid_value);
    var changed = original;
    changed[len] = 1;
    std.mem.writeInt(u64, changed[24..32], len + 1, .little);
    _ = try refuse(changed[0 .. len + 1], o, .invalid_value);
    const page = try find(original[0..len], 0x2005);
    const first = page + 10 + 4; // No tables in this plain fixture.
    inline for (.{ 1, 2, 3, 4, 8 }) |field| {
        changed = original;
        switch (field) {
            1 => changed[first + 1] = 4,
            2 => changed[first + 2] = 3,
            3 => changed[first + 3] = 255,
            4 => std.mem.writeInt(u32, changed[first + 4 ..][0..4], 0x110000, .little),
            8 => std.mem.writeInt(u16, changed[first + 8 ..][0..2], 1, .little),
            else => unreachable,
        }
        repair(changed[0..len], page);
        const failed = try refuse(changed[0..len], o, .invalid_value);
        try testing.expectEqual(@as(u64, 0), failed.heap_bytes + failed.mapped_bytes);
    }
    inline for (.{ "max_physical_slots", "max_rows", "max_pages", "max_records", "max_backing_columns", "max_text_scalars", "max_text_bytes", "max_work_units" }) |field| {
        var reduced = o;
        @field(reduced, field) = 0;
        const failed = try refuse(original[0..len], reduced, .limit_exceeded);
        try testing.expectEqual(@as(u64, 0), failed.heap_bytes + failed.mapped_bytes);
    }
    var output: ct.Terminal = null;
    var report: api.Report = .{};
    try testing.expectEqual(Result.success, api.decode_buf(&original, len, &o, &output, &report));
    ct.free(output);
    const total = report.heap_bytes + report.mapped_bytes;
    var boundary: u64 = 0;
    while (boundary < total) : (boundary += 65536) {
        var reduced = o;
        reduced.max_allocation_bytes = boundary;
        _ = try refuse(original[0..len], reduced, .limit_exceeded);
    }
    var buffer: [64]u8 = @splat(0xa5);
    len = 99;
    try testing.expectEqual(Result.out_of_space, api.encode_buf(source, &o, &buffer, 1, &len, null));
    try testing.expectEqual(@as(usize, 0), len);
    for (buffer) |value| try testing.expectEqual(@as(u8, 0xa5), value);
}
