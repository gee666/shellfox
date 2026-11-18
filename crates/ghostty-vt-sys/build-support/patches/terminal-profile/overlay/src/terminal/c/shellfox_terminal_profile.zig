//! Additive immutable construction/read-only profile ABI.
const std = @import("std");
const lib = @import("../lib.zig");
const terminal = @import("terminal.zig");
const profile = @import("../shellfox_terminal_profile.zig");
const identity = @import("../shellfox_profile.zig");
const Result = @import("result.zig").Result;
pub const State = profile.State;
pub const Options = extern struct {
    size: usize = @sizeOf(Options),
    abi_version: u32 = 1,
    engine_profile: lib.String,
    terminal_profile: u32,
    cols: u16,
    rows: u16,
};
pub fn new_profile(allocator: ?*const lib.alloc.Allocator, out_: ?*terminal.Terminal, options_: ?*const Options) callconv(lib.calling_conv) Result {
    const out = out_ orelse return .invalid_value;
    out.* = null;
    const options = options_ orelse return .invalid_value;
    if (options.size != @sizeOf(Options) or options.abi_version != profile.abi_version or options.engine_profile.len != 64) return .invalid_value;
    if (@intFromPtr(options.engine_profile.ptr) == 0 or !identity.matches(options.engine_profile.ptr[0..64])) return .invalid_value;
    if (options.terminal_profile != profile.Native and options.terminal_profile != profile.LegacyXterm6) return .unsupported;
    // Native-only prerequisite checkpoint. Do not advertise Legacy completion
    // until the standalone-zero layout2/render/codec work is accepted.
    if (options.terminal_profile == profile.LegacyXterm6 and !profile.legacy_live_enabled) return .unsupported;
    const result = terminal.new(allocator, out, options.cols, options.rows);
    if (result != .success) return result;
    profile.initialize(terminal.zigTerminal(out.*).?, options.terminal_profile) catch |err| {
        terminal.free(out.*);
        out.* = null;
        return mapError(err);
    };
    return .success;
}
pub fn get_profile(handle: terminal.Terminal, out_: ?*u32) callconv(lib.calling_conv) Result {
    const out = out_ orelse return .invalid_value;
    const native = terminal.zigTerminal(handle) orelse return .invalid_value;
    if (native.shellfox_terminal_profile != profile.Native and native.shellfox_terminal_profile != profile.LegacyXterm6) return .unsupported;
    out.* = native.shellfox_terminal_profile;
    return .success;
}
pub fn get_profile_state(handle: terminal.Terminal, out_: ?*State) callconv(lib.calling_conv) Result {
    const out = out_ orelse return .invalid_value;
    if (out.size != @sizeOf(State) or out.abi_version != profile.abi_version) return .invalid_value;
    const native = terminal.zigTerminal(handle) orelse return .invalid_value;
    out.* = profile.capture(native) catch |err| return mapError(err);
    return .success;
}
fn mapError(err: profile.Error) Result {
    return switch (err) {
        error.LimitExceeded => .limit_exceeded,
        error.UnsupportedProfile, error.IncompatibleState => .unsupported,
    };
}
test "Shellfox Native profile constructor keeps original raw-v1 bytes and defaults" {
    if (!identity.valid(identity.engine_profile)) return error.SkipZigTest;
    var original: terminal.Terminal = null;
    try std.testing.expectEqual(Result.success, terminal.new(null, &original, 8, 3));
    defer terminal.free(original);
    var selected: terminal.Terminal = null;
    const options: Options = .{ .engine_profile = lib.String.init(identity.engine_profile), .terminal_profile = profile.Native, .cols = 8, .rows = 3 };
    try std.testing.expectEqual(Result.success, new_profile(null, &selected, &options));
    defer terminal.free(selected);
    const cap: usize = 4096;
    for ([_]terminal.Terminal{ original, selected }) |t| {
        try std.testing.expectEqual(Result.success, terminal.set(t, .continuation_max_bytes, &cap));
        const bytes = "A\xcc\x81\xe4\xb8\xad\xf0\x9f\x98\x80\x1b[31";
        terminal.vt_write(t, bytes, bytes.len);
    }
    var a: [65536]u8 = undefined;
    var b: [65536]u8 = undefined;
    var an: usize = 0;
    var bn: usize = 0;
    const snapshot = @import("snapshot.zig");
    try std.testing.expectEqual(Result.success, snapshot.encode_buf(original, &a, a.len, &an));
    try std.testing.expectEqual(Result.success, snapshot.encode_buf(selected, &b, b.len, &bn));
    try std.testing.expectEqualSlices(u8, a[0..an], b[0..bn]);
    var state: State = .{ .terminal_profile = 0, .preceding_join = 99 };
    try std.testing.expectEqual(Result.success, get_profile_state(selected, &state));
    try std.testing.expectEqual(@as(u32, 0), state.preceding_join);
    var unsupported = options;
    unsupported.terminal_profile = profile.LegacyXterm6;
    var missing: terminal.Terminal = null;
    if (!profile.legacy_live_enabled) try std.testing.expectEqual(Result.unsupported, new_profile(null, &missing, &unsupported));
    try std.testing.expectEqual(@as(terminal.Terminal, null), missing);
}
