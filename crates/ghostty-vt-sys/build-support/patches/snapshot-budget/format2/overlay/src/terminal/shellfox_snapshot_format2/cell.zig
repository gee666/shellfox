//! Pointer-free strict cell2 values, never raw-v1 words or invalid-CP sentinels.
const std = @import("std");
const page = @import("../page.zig");
const layout = @import("../shellfox_cell_layout2.zig");
const wire = @import("../snapshot/io.zig");
const guard = @import("../snapshot_budget_guard.zig");
pub const Value = struct {
    kind: u8,
    wide: u8,
    override: u8,
    flags: u8,
    content: u32,
    style: u16,
    link: u16,
    suffix: []const u8,
};
pub fn scalar(cp: u32) !u21 {
    if (cp > 0x10ffff or (cp >= 0xd800 and cp <= 0xdfff)) return error.InvalidScalar;
    return @intCast(cp);
}
pub fn base(value: Value) !page.Cell {
    if (value.kind > 3 or value.wide > 3 or value.override > 2 or value.flags & 0xf8 != 0 or
        (value.kind == 1 and value.content == 0))
        return error.InvalidCell;
    const semantic = std.enums.fromInt(page.Cell.SemanticContent, @as(u2, @truncate(value.flags >> 1))) orelse
        return error.InvalidCell;
    var result: page.Cell = .{ .protected = value.flags & 1 != 0, .semantic_content = semantic };
    switch (value.kind) {
        0, 1 => {
            result.content.codepoint.data = try scalar(value.content);
            result.content_tag = if (value.kind == 1) .codepoint_grapheme else .codepoint;
        },
        2 => {
            if (value.content > 255) return error.InvalidCell;
            result.content_tag = .bg_color_palette;
            result.content = .{ .color_palette = .{ .data = @intCast(value.content) } };
        },
        3 => {
            if (value.content > 0xffffff) return error.InvalidCell;
            result.content_tag = .bg_color_rgb;
            result.content = .{ .color_rgb = .{ .r = @truncate(value.content), .g = @truncate(value.content >> 8), .b = @truncate(value.content >> 16) } };
        },
        else => unreachable,
    }
    try layout.restoreCellLayoutBeforeContinuation(&result, value.wide, value.override);
    return result;
}
pub fn read(r: *std.Io.Reader) !Value {
    var value: Value = .{
        .kind = try r.takeByte(),
        .wide = try r.takeByte(),
        .override = try r.takeByte(),
        .flags = try r.takeByte(),
        .content = try wire.readInt(r, u32),
        .style = try wire.readInt(r, u16),
        .link = try wire.readInt(r, u16),
        .suffix = &.{},
    };
    const count = try wire.readInt(r, u16);
    if (try wire.readInt(r, u16) != 0 or count > page.grapheme_max_len or
        (value.kind == 1) != (count > 0)) return error.InvalidCell;
    value.suffix = try r.take(@as(usize, count) * 4);
    _ = try base(value);
    for (0..count) |i| _ = try scalar(std.mem.readInt(u32, value.suffix[i * 4 ..][0..4], .little));
    try guard.chargeWork(1 + @as(u64, count));
    return value;
}
pub fn encode(w: *std.Io.Writer, p: *const page.Page, cell: *const page.Cell) !void {
    const shape = try layout.cellLayout(cell.*, 1);
    try w.writeByte(@intCast(@backingInt(cell.content_tag)));
    try w.writeByte(shape.native_wide);
    try w.writeByte(shape.semantic_override);
    try w.writeByte(@as(u8, @intFromBool(cell.protected)) | (@as(u8, @intCast(@backingInt(cell.semantic_content))) << 1));
    const content: u32 = switch (cell.content_tag) {
        .codepoint, .codepoint_grapheme => cell.codepoint(),
        .bg_color_palette => cell.content.color_palette.data,
        .bg_color_rgb => @as(u32, cell.content.color_rgb.r) | (@as(u32, cell.content.color_rgb.g) << 8) | (@as(u32, cell.content.color_rgb.b) << 16),
    };
    try wire.writeInt(w, u32, content);
    try wire.writeInt(w, u16, cell.style_id);
    if (cell.hyperlink) try @import("work.zig").sourceLookups(p, 1);
    try wire.writeInt(w, u16, if (cell.hyperlink) p.lookupHyperlink(cell) orelse return error.InvalidCell else 0);
    if (cell.hasGrapheme()) try @import("work.zig").sourceLookups(p, 1);
    const suffix = if (cell.hasGrapheme()) p.lookupGrapheme(cell) orelse return error.InvalidCell else &.{};
    if (suffix.len > page.grapheme_max_len) return error.InvalidCell;
    try wire.writeInt(w, u16, @intCast(suffix.len));
    try wire.writeInt(w, u16, 0);
    for (suffix) |cp| try wire.writeInt(w, u32, cp);
    try guard.chargeWork(1 + suffix.len);
}
pub fn materialize(value: Value, p: *page.Page, row: *page.Row, cell: *page.Cell, style: u16, link: u16) !void {
    var initial = try base(value);
    initial.content_tag = if (value.kind == 1) .codepoint else initial.content_tag;
    initial.style_id = style;
    cell.* = initial;
    if (style != 0) {
        p.styles.use(p.memory, style);
        row.styled = true;
    }
    for (0..value.suffix.len / 4) |i| try p.appendGrapheme(row, cell, try scalar(std.mem.readInt(u32, value.suffix[i * 4 ..][0..4], .little)));
    if (link != 0) {
        p.hyperlink_set.use(p.memory, link);
        try p.setHyperlink(row, cell, link);
    }
    try layout.restoreCellLayoutBeforeContinuation(cell, value.wide, value.override);
}
