//! Strict format2 framing. Payload semantics belong to the canonical codec.
const std = @import("std");
const envelope = @import("envelope.zig");
const guard = @import("../snapshot_budget_guard.zig");
const Crc32c = @import("../../crc32c.zig").Crc32c;

pub const header_len: usize = 10;
pub const Tag = enum(u16) {
    compatibility = 0x2001,
    terminal = 0x2002,
    screen_layout = 0x2003,
    page_layout = 0x2004,
    page_cells = 0x2005,
    continuation = 0x2006,
    finish = 0x2007,
};

pub const Totals = struct {
    records: u64,
    pages: u64,
    physical_slots: u64,
    text_scalars: u64,

    pub fn encode(self: Totals) [32]u8 {
        var bytes: [32]u8 = undefined;
        std.mem.writeInt(u64, bytes[0..8], self.records, .little);
        std.mem.writeInt(u64, bytes[8..16], self.pages, .little);
        std.mem.writeInt(u64, bytes[16..24], self.physical_slots, .little);
        std.mem.writeInt(u64, bytes[24..32], self.text_scalars, .little);
        return bytes;
    }

    pub fn decode(bytes: []const u8) error{InvalidFinish}!Totals {
        if (bytes.len != 32) return error.InvalidFinish;
        return .{
            .records = std.mem.readInt(u64, bytes[0..8], .little),
            .pages = std.mem.readInt(u64, bytes[8..16], .little),
            .physical_slots = std.mem.readInt(u64, bytes[16..24], .little),
            .text_scalars = std.mem.readInt(u64, bytes[24..32], .little),
        };
    }
};

pub const Limits = struct {
    encoded_bytes: u64,
    records: u64,
    pages: u64,
    screens: u32,
};

pub const Frame = struct { tag: Tag, payload: []const u8 };
pub const Reader = struct {
    bytes: []const u8,
    offset: usize = envelope.len,
    scope: *guard.Scope,
    limits: Limits,
    records: u64 = 0,
    pages: u64 = 0,
    screens: u32 = 0,
    screen_has_page: bool = false,
    pending_cells: bool = false,
    totals: ?Totals = null,
    phase: enum { compatibility, terminal, layouts, finish, complete, failed } = .compatibility,

    pub fn init(bytes: []const u8, layout_abi: u32, limits: Limits, scope: *guard.Scope) !Reader {
        _ = try envelope.decode(bytes, layout_abi, limits.encoded_bytes);
        try scope.chargeWork(envelope.len);
        return .{ .bytes = bytes, .scope = scope, .limits = limits };
    }

    pub fn next(self: *Reader) !?Frame {
        if (self.phase == .failed) return error.ReaderFailed;
        if (self.phase == .complete) return null;
        errdefer self.phase = .failed;
        if (self.records >= self.limits.records) return error.LimitExceeded;
        if (self.bytes.len - self.offset < header_len) return error.Truncated;
        const header = self.bytes[self.offset..][0..header_len];
        const raw_tag = std.mem.readInt(u16, header[0..2], .little);
        const tag = std.enums.fromInt(Tag, raw_tag) orelse return error.UnsupportedRecord;
        const size: usize = std.mem.readInt(u32, header[2..6], .little);
        if (size > self.bytes.len - self.offset - header_len) return error.Truncated;
        try self.scope.chargeWork(header_len + 2 * @as(u64, size));
        const payload = self.bytes[self.offset + header_len ..][0..size];
        var crc = Crc32c.init();
        crc.update(header[0..6]);
        crc.update(payload);
        if (crc.final() != std.mem.readInt(u32, header[6..10], .little)) return error.InvalidChecksum;
        self.records += 1;
        self.offset += header_len + size;
        try self.route(tag, payload);
        return .{ .tag = tag, .payload = payload };
    }

    fn route(self: *Reader, tag: Tag, payload: []const u8) !void {
        switch (self.phase) {
            .compatibility => {
                if (tag != .compatibility) return error.InvalidOrder;
                self.phase = .terminal;
            },
            .terminal => {
                if (tag != .terminal) return error.InvalidOrder;
                self.phase = .layouts;
            },
            .layouts => switch (tag) {
                .screen_layout => {
                    if (self.pending_cells or (self.screens > 0 and !self.screen_has_page)) return error.InvalidOrder;
                    if (self.screens >= self.limits.screens) return error.LimitExceeded;
                    self.screens += 1;
                    self.screen_has_page = false;
                },
                .page_layout => {
                    if (self.screens == 0 or self.pending_cells) return error.InvalidOrder;
                    if (self.pages >= self.limits.pages) return error.LimitExceeded;
                    self.pages += 1;
                    self.pending_cells = true;
                },
                .page_cells => {
                    if (!self.pending_cells) return error.InvalidOrder;
                    self.pending_cells = false;
                    self.screen_has_page = true;
                },
                .continuation => {
                    if (self.screens == 0 or self.pending_cells or !self.screen_has_page) return error.InvalidOrder;
                    self.phase = .finish;
                },
                else => return error.InvalidOrder,
            },
            .finish => {
                if (tag != .finish) return error.InvalidOrder;
                const total = try Totals.decode(payload);
                if (total.records != self.records or total.pages != self.pages) return error.InvalidFinish;
                if (self.offset != self.bytes.len) return error.TrailingBytes;
                self.totals = total;
                self.phase = .complete;
            },
            .complete, .failed => return error.ReaderFailed,
        }
    }

    /// Must be called with measured canonical payload totals before publication.
    pub fn verifyTotals(self: *Reader, actual: Totals) !void {
        errdefer self.phase = .failed;
        if (self.phase != .complete) return error.MissingFinish;
        const declared = self.totals orelse return error.MissingFinish;
        if (actual.records != declared.records or actual.pages != declared.pages or
            actual.physical_slots != declared.physical_slots or actual.text_scalars != declared.text_scalars)
            return error.InvalidFinish;
    }
};

/// Writes only into a caller-selected private stage, never a public artifact.
pub fn write(writer: *std.Io.Writer, tag: Tag, payload: []const u8) !void {
    const size = std.math.cast(u32, payload.len) orelse return error.LimitExceeded;
    var header: [header_len]u8 = undefined;
    std.mem.writeInt(u16, header[0..2], @backingInt(tag), .little);
    std.mem.writeInt(u32, header[2..6], size, .little);
    var crc = Crc32c.init();
    crc.update(header[0..6]);
    crc.update(payload);
    std.mem.writeInt(u32, header[6..10], crc.final(), .little);
    try writer.writeAll(&header);
    try writer.writeAll(payload);
}
