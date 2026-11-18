//! Format2 discriminator. Checked before records, cells or native allocation.
const std = @import("std");

pub const magic: [8]u8 = .{ 'S', 'F', 'O', 'X', 'S', 'N', '2', 0 };
pub const version: u16 = 2;
pub const wire_profile: u32 = 2;
pub const len: usize = 32;

pub const Error = error{
    InvalidMagic,
    Truncated,
    UnsupportedVersion,
    UnsupportedPrefix,
    UnsupportedProfile,
    UnsupportedLayoutABI,
    UnsupportedFlags,
    EncodedLengthMismatch,
    LimitExceeded,
};

pub const Header = struct {
    layout_abi: u32,
    encoded_len: u64,
};

pub fn encode(header: Header) Error![len]u8 {
    if (header.layout_abi == 0) return error.UnsupportedLayoutABI;
    if (header.encoded_len < len) return error.EncodedLengthMismatch;
    var bytes: [len]u8 = @splat(0);
    @memcpy(bytes[0..8], &magic);
    std.mem.writeInt(u16, bytes[8..10], version, .little);
    std.mem.writeInt(u16, bytes[10..12], len, .little);
    std.mem.writeInt(u32, bytes[12..16], wire_profile, .little);
    std.mem.writeInt(u32, bytes[16..20], header.layout_abi, .little);
    std.mem.writeInt(u64, bytes[24..32], header.encoded_len, .little);
    return bytes;
}

/// This is prefix admission only. It does not claim record/layout validity.
/// expected_layout_abi comes from the actual compiled canonical core contract.
pub fn decode(bytes: []const u8, expected_layout_abi: u32, max_encoded_bytes: u64) Error!Header {
    if (bytes.len > max_encoded_bytes) return error.LimitExceeded;
    if (bytes.len < len) return error.Truncated;
    if (!std.mem.eql(u8, bytes[0..8], &magic)) return error.InvalidMagic;
    if (std.mem.readInt(u16, bytes[8..10], .little) != version) return error.UnsupportedVersion;
    if (std.mem.readInt(u16, bytes[10..12], .little) != len) return error.UnsupportedPrefix;
    if (std.mem.readInt(u32, bytes[12..16], .little) != wire_profile) return error.UnsupportedProfile;
    const layout_abi = std.mem.readInt(u32, bytes[16..20], .little);
    if (layout_abi == 0 or expected_layout_abi == 0 or layout_abi != expected_layout_abi)
        return error.UnsupportedLayoutABI;
    if (std.mem.readInt(u32, bytes[20..24], .little) != 0) return error.UnsupportedFlags;
    const encoded_len = std.mem.readInt(u64, bytes[24..32], .little);
    if (encoded_len != bytes.len) return error.EncodedLengthMismatch;
    return .{ .layout_abi = layout_abi, .encoded_len = encoded_len };
}

test "format2 prefix golden and no raw1 discriminator reuse" {
    const expected = "SFOXSN2\x00\x02\x00\x20\x00\x02\x00\x00\x00" ++
        "\x04\x03\x02\x01\x00\x00\x00\x00\x20\x00\x00\x00\x00\x00\x00\x00";
    const header = try encode(.{ .layout_abi = 0x01020304, .encoded_len = len });
    try std.testing.expectEqualSlices(u8, expected, &header);
    try std.testing.expectEqual(@as(u32, 0x01020304), (try decode(&header, 0x01020304, len)).layout_abi);
    var raw1: std.Io.Reader = .fixed(&header);
    try std.testing.expectError(error.InvalidMagic, @import("../snapshot/envelope.zig").decode(&raw1));
    var old: [len]u8 = @splat(0);
    @memcpy(old[0..8], "GHOSTSNP");
    std.mem.writeInt(u16, old[8..10], 1, .little);
    try std.testing.expectError(error.InvalidMagic, decode(&old, 0x01020304, len));
}

test "format2 prefix truncation versions ABI flags and exact length" {
    const header = try encode(.{ .layout_abi = 7, .encoded_len = len });
    for (0..len) |size| try std.testing.expectError(error.Truncated, decode(header[0..size], 7, len));
    try std.testing.expectError(error.LimitExceeded, decode(&header, 7, len - 1));
    try std.testing.expectError(error.UnsupportedLayoutABI, decode(&header, 0, len));
    try std.testing.expectError(error.UnsupportedLayoutABI, decode(&header, 8, len));
    inline for (.{ 8, 10, 12, 16, 20, 24 }) |offset| {
        var changed = header;
        changed[offset] ^= 1;
        if (decode(&changed, 7, len)) |_| return error.TestUnexpectedResult else |_| {}
    }
}
