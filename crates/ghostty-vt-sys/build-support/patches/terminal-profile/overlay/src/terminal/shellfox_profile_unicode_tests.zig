//! Differential assertion over the ONE attributed module supplied by ANSI patch1.
const std = @import("std");
const v6 = @import("shellfox_unicode_v6.zig");
test "Shellfox shared UnicodeV6 all codepoints and reachable preceding properties match shipped xterm6" {
    var hash = std.crypto.hash.sha2.Sha256.init(.{});
    for (0..0x110000) |value| {
        const cp: u21 = @intCast(value);
        hash.update(&.{ v6.width(cp), @intCast(v6.properties(cp, 0).?), @intCast(v6.properties(cp, 2).?), @intCast(v6.properties(cp, 3).?), @intCast(v6.properties(cp, 4).?), @intCast(v6.properties(cp, 5).?) });
    }
    var actual: [32]u8 = undefined;
    hash.final(&actual);
    var expected: [32]u8 = undefined;
    _ = try std.fmt.hexToBytes(&expected, "817ebb04d065be74f5c9c9f86940a9bda10931d27803bd1722e0415c5f1abeb3");
    try std.testing.expectEqualSlices(u8, &expected, &actual);
}
