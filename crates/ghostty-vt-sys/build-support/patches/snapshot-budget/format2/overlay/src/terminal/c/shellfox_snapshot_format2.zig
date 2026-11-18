//! Experimental additive format2 C boundary. Normal Legacy availability is OFF.
const std = @import("std");
const lib = @import("../lib.zig");
const ct = @import("terminal.zig");
const Result = @import("result.zig").Result;
const codec = @import("../shellfox_snapshot_format2/codec.zig");
const guard = @import("../snapshot_budget_guard.zig");
const schema = @import("../shellfox_snapshot_format2/options.zig");
pub const Options = schema.Options;
pub const Report = schema.Report;
fn configuration(o: ?*const Options, report: ?*Report) !Options {
    const ptr = o orelse return error.InvalidOptions;
    if (ptr.size != @sizeOf(Options)) return error.InvalidOptions;
    if (report) |r| {
        if (r.size != @sizeOf(Report) or r.abi_version != 1 or r.reserved != 0) return error.InvalidOptions;
        r.* = .{};
    }
    try schema.validate(ptr.*);
    return ptr.*;
}
fn map(err: anyerror, scope: *const guard.Scope) Result {
    if (scope.exceeded) return .limit_exceeded;
    if (scope.allocation_failed) return .out_of_memory;
    return switch (err) {
        error.OutOfMemory => .out_of_memory,
        error.OutOfSpace => .out_of_space,
        error.LimitExceeded, error.EncodedLimitExceeded, error.ContinuationLimitExceeded => .limit_exceeded,
        error.UnsupportedProfile, error.UnsupportedVersion, error.UnsupportedLayoutABI, error.UnsupportedFlags, error.UnsupportedPrefix, error.UnsupportedRecord => .unsupported,
        else => .invalid_value,
    };
}
fn publishReport(report: ?*Report, scope: guard.Scope, offset: usize) void {
    if (report) |r| {
        r.source_offset = offset;
        r.heap_bytes = scope.heap_bytes;
        r.mapped_bytes = scope.mapped_bytes;
        r.work_units = scope.work;
    }
}
pub fn encode_buf(source: ct.Terminal, options_: ?*const Options, output: ?[*]u8, capacity: usize, written_: ?*usize, report: ?*Report) callconv(lib.calling_conv) Result {
    const written = written_ orelse return .invalid_value;
    written.* = 0;
    if (comptime !lib.feature(.snapshot) or !codec.supported_build) return .unsupported;
    if (!codec.enabled()) return .unsupported;
    const o = configuration(options_, report) catch |err| return if (err == error.UnsupportedProfile) .unsupported else .invalid_value;
    const ptr = output orelse return .invalid_value;
    var scope: guard.Scope = .{ .max_bytes = o.max_allocation_bytes, .max_work = o.max_work_units };
    if (!guard.enter(&scope)) return .unsupported;
    defer guard.leave();
    defer publishReport(report, scope, 0);
    const body = codec.encode(source, o, &scope, capacity) catch |err| {
        return map(err, &scope);
    };
    defer body.deinit();
    if (scope.failed()) return map(error.OutOfMemory, &scope);
    if (report) |r| {
        r.physical_slots = body.totals.physical_slots;
        r.text_scalars = body.totals.text_scalars;
    }
    @memcpy(ptr[0..body.len], body.bytes());
    written.* = body.len;
    return .success;
}
pub fn decode_buf(input: ?[*]const u8, len: usize, options_: ?*const Options, out_: ?*ct.Terminal, report: ?*Report) callconv(lib.calling_conv) Result {
    const out = out_ orelse return .invalid_value;
    out.* = null;
    if (comptime !lib.feature(.snapshot) or !codec.supported_build) return .unsupported;
    if (!codec.enabled()) return .unsupported;
    const o = configuration(options_, report) catch |err| return if (err == error.UnsupportedProfile) .unsupported else .invalid_value;
    const bytes: []const u8 = if (input) |p| p[0..len] else if (len == 0) &.{} else return .invalid_value;
    var scope: guard.Scope = .{ .max_bytes = o.max_allocation_bytes, .max_work = o.max_work_units };
    if (!guard.enter(&scope)) return .unsupported;
    defer guard.leave();
    var offset: usize = 0;
    defer publishReport(report, scope, offset);
    const decoded = codec.decode(bytes, o, &scope) catch |err| {
        return map(err, &scope);
    };
    if (scope.failed()) {
        ct.free(decoded.terminal);
        return map(error.OutOfMemory, &scope);
    }
    offset = len;
    if (report) |r| {
        r.physical_slots = decoded.totals.physical_slots;
        r.text_scalars = decoded.totals.text_scalars;
    }
    out.* = decoded.terminal;
    return .success;
}

test "format2 review unsupported builds refuse before source or allocation" {
    if (comptime codec.supported_build) return error.SkipZigTest;
    var out: ct.Terminal = null;
    var written: usize = 99;
    try std.testing.expectEqual(Result.unsupported, encode_buf(null, null, null, 0, &written, null));
    try std.testing.expectEqual(@as(usize, 0), written);
    try std.testing.expectEqual(Result.unsupported, decode_buf(null, 0, null, &out, null));
    try std.testing.expectEqual(@as(ct.Terminal, null), out);
}

test {
    _ = @import("../shellfox_snapshot_format2/framing_tests.zig");
    _ = @import("../shellfox_snapshot_format2/codec_tests.zig");
    _ = @import("../shellfox_snapshot_format2/matrix_tests.zig");
    _ = @import("../shellfox_snapshot_format2/failure_tests.zig");
    _ = @import("../shellfox_snapshot_format2/review_tests.zig");
    _ = @import("../shellfox_snapshot_format2/managed_review_tests.zig");
}
