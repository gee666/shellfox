const std = @import("std");
const Terminal = @import("Terminal.zig");
const profile = @import("shellfox_terminal_profile.zig");
const v6 = @import("shellfox_unicode_v6.zig");
fn make() !Terminal {
    return Terminal.init(std.testing.io, std.testing.allocator, .{ .cols = 8, .rows = 3 });
}
test "Shellfox profile State one sized checked ABI and Native zero legacy state" {
    var native: profile.State = .{ .terminal_profile = profile.Native, .preceding_join = 0 };
    try profile.validateState(&native, 0);
    native.preceding_join = 2;
    try std.testing.expectError(error.IncompatibleState, profile.validateState(&native, 0));
    for ([_]u32{ 0, 2, 3, 4, 5 }) |value| {
        const state: profile.State = .{ .terminal_profile = profile.LegacyXterm6, .preceding_join = value };
        try profile.validateState(&state, 1);
    }
    for ([_]u32{ 1, 6, 7, 8, 0xffffffff }) |value| {
        const state: profile.State = .{ .terminal_profile = profile.LegacyXterm6, .preceding_join = value };
        try std.testing.expectError(error.IncompatibleState, profile.validateState(&state, 1));
    }
    native.preceding_join = 0;
    native.size -= 1;
    try std.testing.expectError(error.IncompatibleState, profile.validateState(&native, 0));
    native.size = @sizeOf(profile.State);
    native.abi_version = 2;
    try std.testing.expectError(error.IncompatibleState, profile.validateState(&native, 0));
}
test "Shellfox Native restoration has bounded validation and no guessed Legacy state" {
    var terminal = try make();
    defer terminal.deinit(std.testing.allocator);
    const before = try profile.capture(&terminal);
    try std.testing.expectEqual(@as(u32, 0), before.terminal_profile);
    try std.testing.expectError(error.LimitExceeded, profile.restoreStateBeforeContinuation(&terminal, 0, null, 23));
    try profile.restoreStateBeforeContinuation(&terminal, 0, null, 24);
    try profile.applyRestored(&terminal, 0, 24);
    try std.testing.expectError(error.UnsupportedProfile, profile.restoreStateBeforeContinuation(&terminal, 99, null, 24));
    if (!profile.legacy_live_enabled) try std.testing.expectError(error.UnsupportedProfile, profile.restoreStateBeforeContinuation(&terminal, 1, null, 24));
    try std.testing.expectEqual(before, try profile.capture(&terminal));
}
test "Shellfox Legacy prototype exact base combining ZWJ VS16 width and REP state" {
    var terminal = try make();
    defer terminal.deinit(std.testing.allocator);
    try profile.initialize(&terminal, 1);
    try terminal.print('A');
    try terminal.print(0x0301);
    try terminal.print(0xfe0f);
    try terminal.print(0x200d);
    try std.testing.expectEqual(@as(u32, 3), terminal.shellfox_preceding_join);
    try std.testing.expectEqual(@as(u16, 1), terminal.screens.active.cursor.x);
    try profile.repeat(&terminal, 1);
    try std.testing.expectEqual(@as(u16, 2), terminal.screens.active.cursor.x);
    const text = try terminal.plainString(std.testing.allocator);
    defer std.testing.allocator.free(text);
    try std.testing.expectEqualStrings("A\xcc\x81\xef\xb8\x8f\xe2\x80\x8dA\xcc\x81\xef\xb8\x8f\xe2\x80\x8d", text);
    profile.boundary(&terminal);
    try std.testing.expectEqual(@as(u32, 0), terminal.shellfox_preceding_join);
    try profile.repeat(&terminal, 3);
    try std.testing.expectEqual(@as(u16, 2), terminal.screens.active.cursor.x);
    try std.testing.expectEqual(@as(u2, 1), v6.width(0x1f600));
    try terminal.print(0x1f600);
    try std.testing.expectEqual(@as(u16, 3), terminal.screens.active.cursor.x);
}
test "Shellfox standalone-zero measured gap is failclosed not a lossy success" {
    var terminal = try make();
    defer terminal.deinit(std.testing.allocator);
    try profile.initialize(&terminal, 1);
    try std.testing.expectError(error.UnsupportedTerminalProfile, terminal.print(0x0301));
    try std.testing.expect(terminal.shellfox_profile_incompatible);
    try std.testing.expectError(error.IncompatibleState, profile.capture(&terminal));
}
