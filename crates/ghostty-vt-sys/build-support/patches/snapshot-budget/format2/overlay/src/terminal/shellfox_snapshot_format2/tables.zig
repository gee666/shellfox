const std = @import("std");
const page = @import("../page.zig");
const hyperlink = @import("../hyperlink.zig");
const style = @import("../snapshot/style.zig");
const links = @import("../snapshot/hyperlink.zig");
const wire = @import("../snapshot/io.zig");
const guard = @import("../snapshot_budget_guard.zig");

pub fn readLink(r: *std.Io.Reader) !hyperlink.Hyperlink {
    const kind = try r.takeByte();
    const id: hyperlink.Hyperlink.Id = switch (kind) {
        1 => .{ .implicit = try wire.readInt(r, u32) },
        2 => .{ .explicit = try r.take(try wire.readInt(r, u32)) },
        else => return error.InvalidHyperlink,
    };
    const uri = try r.take(try wire.readInt(r, u32));
    if (uri.len == 0 or (id == .explicit and id.explicit.len == 0)) return error.InvalidHyperlink;
    try guard.chargeWork(uri.len + if (id == .explicit) id.explicit.len else @as(usize, 0));
    return .{ .id = id, .uri = uri };
}
pub fn strictStyle(r: *std.Io.Reader) !@import("../style.zig").Style {
    const encoded = try r.take(style.len);
    const parsed = style.parseOrNull(encoded[0..style.len]) orelse return error.InvalidStyle;
    var buffer: [style.len]u8 = undefined;
    var w: std.Io.Writer = .fixed(&buffer);
    try style.encode(parsed, &w);
    if (!std.mem.eql(u8, encoded, &buffer)) return error.InvalidStyle;
    return parsed;
}

fn sameLink(a: hyperlink.Hyperlink, b: hyperlink.Hyperlink) !bool {
    const bytes = std.math.add(u64, a.uri.len, b.uri.len) catch return error.LimitExceeded;
    const ids = std.math.add(u64, if (a.id == .explicit) a.id.explicit.len else 0, if (b.id == .explicit) b.id.explicit.len else 0) catch return error.LimitExceeded;
    try guard.chargeWork(std.math.add(u64, bytes, ids) catch return error.LimitExceeded);
    if (std.meta.activeTag(a.id) != std.meta.activeTag(b.id)) return false;
    const equal_id = switch (a.id) {
        .implicit => |id| id == b.id.implicit,
        .explicit => |id| std.mem.eql(u8, id, b.id.explicit),
    };
    return equal_id and std.mem.eql(u8, a.uri, b.uri);
}
/// Complete canonicality pass before ANY native reference increment. Distinct
/// wire IDs cannot alias a native deduplicated entry and accumulate staging refs.
fn canonical(r: *std.Io.Reader) !void {
    try guard.chargeWork(16384);
    var styles: std.StaticBitSet(65536) = .empty;
    var hyperlinks: std.StaticBitSet(65536) = .empty;
    const count = try wire.readInt(r, u16);
    const link_count = try wire.readInt(r, u16);
    const style_bytes = r.buffered();
    for (0..count) |i| {
        const id = try wire.readInt(r, u16);
        if (id == 0 or styles.isSet(id)) return error.InvalidStyle;
        styles.set(id);
        const value = try strictStyle(r);
        if (value.default()) return error.InvalidStyle;
        for (0..i) |j| {
            try guard.chargeWork(style.len);
            var prior: std.Io.Reader = .fixed(style_bytes[j * (style.len + 2) + 2 ..][0..style.len]);
            if (value.eql(try strictStyle(&prior))) return error.NoncanonicalStyleAlias;
        }
    }
    const link_bytes = r.buffered();
    for (0..link_count) |i| {
        const id = try wire.readInt(r, u16);
        if (id == 0 or hyperlinks.isSet(id)) return error.InvalidHyperlink;
        hyperlinks.set(id);
        const value = try readLink(r);
        var prior: std.Io.Reader = .fixed(link_bytes);
        for (0..i) |_| {
            _ = try wire.readInt(&prior, u16);
            if (try sameLink(value, try readLink(&prior))) return error.NoncanonicalHyperlinkAlias;
        }
    }
}

pub const Decoded = struct {
    style_seen: std.StaticBitSet(65536) = .empty,
    link_seen: std.StaticBitSet(65536) = .empty,
    styles: []u16 = &.{},
    links: []u16 = &.{},
    alloc: std.mem.Allocator,
    pub fn deinit(self: *Decoded) void {
        self.alloc.free(self.styles);
        self.alloc.free(self.links);
    }
    pub fn releaseStaging(self: *Decoded, p: *page.Page) void {
        var it = self.style_seen.iterator(.{});
        while (it.next()) |id| p.styles.release(p.memory, self.styles[id]);
        it = self.link_seen.iterator(.{});
        while (it.next()) |id| p.hyperlink_set.release(p.memory, self.links[id]);
    }
};
pub fn decode(r: *std.Io.Reader, target: ?*page.Page, tally: ?*@import("resources.zig").Tally, capacity_bytes: u64) !Decoded {
    var check: std.Io.Reader = .fixed(r.buffered());
    try canonical(&check);
    const alloc = guard.heapAllocator();
    try guard.chargeWork(16384); // Both fixed encoded-ID presence bitsets.
    var result: Decoded = .{ .alloc = alloc };
    errdefer result.deinit();
    const count = try wire.readInt(r, u16);
    const link_count = try wire.readInt(r, u16);
    if (target != null) {
        result.styles = try alloc.alloc(u16, 65536);
        result.links = try alloc.alloc(u16, 65536);
        @memset(result.styles, 0);
        @memset(result.links, 0);
    }
    for (0..count) |_| {
        const id = try wire.readInt(r, u16);
        if (id == 0 or result.style_seen.isSet(id)) return error.InvalidStyle;
        result.style_seen.set(id);
        const value = try strictStyle(r);
        if (value.default()) return error.InvalidStyle;
        try @import("work.zig").native(capacity_bytes, 1, style.len);
        if (target) |p| result.styles[id] = try p.styles.add(p.memory, value);
        try guard.chargeWork(style.len);
    }
    for (0..link_count) |_| {
        const id = try wire.readInt(r, u16);
        if (id == 0 or result.link_seen.isSet(id)) return error.InvalidHyperlink;
        result.link_seen.set(id);
        const value = try readLink(r);
        if (tally) |t| try t.text(0, value.uri.len + if (value.id == .explicit) value.id.explicit.len else @as(usize, 0));
        const bytes = std.math.add(u64, value.uri.len, if (value.id == .explicit) value.id.explicit.len else 0) catch return error.LimitExceeded;
        try @import("work.zig").native(capacity_bytes, 1, std.math.add(u64, bytes, 1) catch return error.LimitExceeded);
        if (target) |p| result.links[id] = try p.insertHyperlink(value);
    }
    return result;
}
pub fn encode(w: *std.Io.Writer, p: *const page.Page) !void {
    try wire.writeInt(w, u16, std.math.cast(u16, p.styles.count()) orelse return error.LimitExceeded);
    try wire.writeInt(w, u16, std.math.cast(u16, p.hyperlink_set.count()) orelse return error.LimitExceeded);
    var styles = p.styles.iterator(p.memory);
    while (styles.next()) |entry| {
        try wire.writeInt(w, u16, entry.id);
        try style.encode(entry.value_ptr.*, w);
        try guard.chargeWork(style.len);
    }
    var it = p.hyperlink_set.iterator(p.memory);
    while (it.next()) |entry| {
        const value = entry.value_ptr;
        try wire.writeInt(w, u16, entry.id);
        try links.encode(.{
            .uri = value.uri.slice(p.memory),
            .id = switch (value.id) {
                .implicit => |id| .{ .implicit = id },
                .explicit => |id| .{ .explicit = id.slice(p.memory) },
            },
        }, w);
    }
}
