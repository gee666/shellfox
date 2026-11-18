const std = @import("std");
const testing = std.testing;
const api = @import("../c/shellfox_snapshot_format2.zig");
const ct = @import("../c/terminal.zig");
const fixtures = @import("codec_tests.zig");
const compare = @import("compare_tests.zig");
const Result = @import("../c/result.zig").Result;
const Step = struct { hex: ?[]const u8 = null, resize: ?[2]u16 = null };
const Case = struct { name: []const u8, cols: u16, rows: u16, steps: []Step };
fn apply(t: ct.Terminal, step: Step) !void {
    if (step.hex) |hex| {
        const bytes = try testing.allocator.alloc(u8, hex.len / 2);
        defer testing.allocator.free(bytes);
        _ = try std.fmt.hexToBytes(bytes, hex);
        ct.vt_write(t, bytes.ptr, bytes.len);
    }
    if (step.resize) |size| try testing.expectEqual(Result.success, ct.resize(t, size[0], size[1], 0, 0));
}
test "format2 codec physical hidden storage edit reflow and live suffix matrix" {
    // The exact published P4 oracle cases, without duplicating its parser/table.
    const parsed = try std.json.parseFromSlice([]Case, testing.allocator, @embedFile("../shellfox_cell_layout2_cases.json"), .{ .ignore_unknown_fields = true });
    defer parsed.deinit();
    const bytes = try testing.allocator.alloc(u8, 1024 * 1024);
    defer testing.allocator.free(bytes);
    const o = fixtures.options();
    var cuts: usize = 0;
    for (parsed.value) |case| {
        const original = try fixtures.sourceSized(case.cols, case.rows);
        defer ct.free(original);
        var previous: ct.Terminal = null;
        defer ct.free(previous);
        for (case.steps, 0..) |step, index| {
            try apply(original, step);
            if (previous != null) {
                try apply(previous, step);
                compare.terminals(original, previous) catch |err| {
                    std.debug.print("format2 suffix {s} cut{d}: {s}\n", .{ case.name, index, @errorName(err) });
                    return err;
                };
                ct.free(previous);
                previous = null;
            }
            var len: usize = 0;
            const encoded = api.encode_buf(original, &o, bytes.ptr, bytes.len, &len, null);
            if (encoded != .success) std.debug.print("format2 encode {s} cut{d}\n", .{ case.name, index });
            try testing.expectEqual(Result.success, encoded);
            const decoded = api.decode_buf(bytes.ptr, len, &o, &previous, null);
            if (decoded != .success) std.debug.print("format2 decode {s} cut{d}\n", .{ case.name, index });
            try testing.expectEqual(Result.success, decoded);
            compare.terminals(original, previous) catch |err| {
                std.debug.print("format2 roundtrip {s} cut{d}: {s}\n", .{ case.name, index, @errorName(err) });
                return err;
            };
            cuts += 1;
        }
        ct.vt_write(original, "!", 1);
        ct.vt_write(previous, "!", 1);
        try compare.terminals(original, previous);
    }
    try testing.expect(cuts >= 214);
}
