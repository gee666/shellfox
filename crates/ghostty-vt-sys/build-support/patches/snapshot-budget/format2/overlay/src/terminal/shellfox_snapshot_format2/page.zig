const std = @import("std");
const native = @import("../page.zig");
const Screen = @import("../Screen.zig");
const layout = @import("../shellfox_cell_layout2.zig");
const guard = @import("../snapshot_budget_guard.zig");
const wire = @import("../snapshot/io.zig");
const cells = @import("cell.zig");
const tables = @import("tables.zig");
const resources = @import("resources.zig");

pub const Description = struct {
    cols: u16,
    rows: u16,
    capacity: native.Capacity,
    row_data: []const u8,
};
pub fn describe(bytes: []const u8, visible: u16, tally: ?*resources.Tally) !Description {
    var r: std.Io.Reader = .fixed(bytes);
    const cols = try wire.readInt(&r, u16);
    const rows = try wire.readInt(&r, u16);
    const cap: native.Capacity = .{
        .cols = try wire.readInt(&r, u16),
        .rows = try wire.readInt(&r, u16),
        .styles = try wire.readInt(&r, u16),
        .hyperlink_bytes = try wire.readInt(&r, u16),
        .hyperlink_cells = try wire.readInt(&r, u16),
        .grapheme_bytes = try wire.readInt(&r, u32),
        .string_bytes = try wire.readInt(&r, u32),
    };
    if (cols == 0 or rows == 0 or cap.cols < cols or cap.rows < rows or cap.cols > 4096 or
        @as(u64, cap.rows) * cap.cols > guard.max_page_cells or cap.grapheme_bytes > 64 * 1024 * 1024 or
        cap.string_bytes > 64 * 1024 * 1024) return error.LimitExceeded;
    const row_data = try r.take(@as(usize, rows) * 4);
    if (r.bufferedLen() != 0) return error.InvalidPage;
    if (tally) |t| try t.page(visible, cols, rows);
    for (0..rows) |y| {
        const value = row_data[y * 4 ..][0..4];
        const backing = std.mem.readInt(u16, value[0..2], .little);
        if (backing < visible or backing > cols or value[2] & 0xf0 != 0 or value[3] != 0 or
            std.enums.fromInt(@FieldType(native.Row, "semantic_prompt"), @as(u2, @truncate(value[2] >> 2))) == null)
            return error.InvalidRow;
        if (tally) |t| try t.row(visible, cols, backing);
    }
    try guard.chargeWork(@as(u64, cap.rows) * cap.cols);
    return .{ .cols = cols, .rows = rows, .capacity = cap, .row_data = row_data };
}
pub fn encodeLayout(w: *std.Io.Writer, s: *const Screen, p: *const native.Page) !void {
    try wire.writeInt(w, u16, p.size.cols);
    try wire.writeInt(w, u16, p.size.rows);
    try wire.writeInt(w, u16, p.capacity.cols);
    try wire.writeInt(w, u16, p.capacity.rows);
    try wire.writeInt(w, u16, p.capacity.styles);
    try wire.writeInt(w, u16, p.capacity.hyperlink_bytes);
    try wire.writeInt(w, u16, p.capacity.hyperlink_cells);
    try wire.writeInt(w, u32, p.capacity.grapheme_bytes);
    try wire.writeInt(w, u32, p.capacity.string_bytes);
    for (0..p.size.rows) |y| {
        const row = p.getRow(y);
        if (row.kitty_virtual_placeholder) return error.UnsupportedProfile;
        try wire.writeInt(w, u16, try layout.rowBackingCols(s, p, row));
        try w.writeByte(@as(u8, @intFromBool(row.wrap)) | (@as(u8, @intFromBool(row.wrap_continuation)) << 1) |
            (@as(u8, @intCast(@backingInt(row.semantic_prompt))) << 2));
        try w.writeByte(0);
    }
}
pub fn encodeCells(w: *std.Io.Writer, p: *const native.Page) !void {
    try tables.encode(w, p);
    for (0..p.size.rows) |y| for (p.getCells(p.getRow(y))) |*cell| try cells.encode(w, p, cell);
}

/// Run with target=null during complete preallocation admission; materialize
/// only after the whole envelope passed and the screen geometry was staged.
pub fn decodeCells(bytes: []const u8, d: Description, s: ?*Screen, target: ?*native.Page, tally: ?*resources.Tally) !void {
    var r: std.Io.Reader = .fixed(bytes);
    const capacity_bytes = native.Page.layout(d.capacity).total_size;
    var refs = try tables.decode(&r, target, tally, capacity_bytes);
    defer refs.deinit();
    if (target) |p| {
        p.pauseIntegrityChecks(true);
        defer p.pauseIntegrityChecks(false);
        try fill(&r, d, s.?, p, &refs, tally);
        refs.releaseStaging(p);
    } else try fill(&r, d, null, null, &refs, tally);
    if (r.bufferedLen() != 0) return error.InvalidPage;
}
fn fill(r: *std.Io.Reader, d: Description, screen: ?*Screen, target: ?*native.Page, refs: *tables.Decoded, tally: ?*resources.Tally) !void {
    for (0..d.rows) |y| {
        const meta = d.row_data[y * 4 ..][0..4];
        const backing = std.mem.readInt(u16, meta[0..2], .little);
        if (target) |p| {
            const row = p.getRow(y);
            row.wrap = meta[2] & 1 != 0;
            row.wrap_continuation = meta[2] & 2 != 0;
            row.semantic_prompt = std.enums.fromInt(@FieldType(native.Row, "semantic_prompt"), @as(u2, @truncate(meta[2] >> 2))) orelse return error.InvalidRow;
            try layout.restoreRowBackingBeforeContinuation(screen.?, p, row, backing);
        }
        var pending_wide = false;
        for (0..d.cols) |x| {
            const value = try cells.read(r);
            if ((value.style != 0 and !refs.style_seen.isSet(value.style)) or
                (value.link != 0 and !refs.link_seen.isSet(value.link))) return error.InvalidReference;
            const capacity_bytes = native.Page.layout(d.capacity).total_size;
            // Prepay native maps/bitmap scans before materialization, including
            // in the allocation-free admission pass. Core layout is validated
            // again before and after continuation, with managed suffix lookups.
            try @import("work.zig").managed(capacity_bytes, value.suffix.len / 4, value.link != 0);
            try @import("work.zig").native(capacity_bytes, 2, 8 + value.suffix.len);
            const probe = try cells.base(value);
            if (x >= backing and (!probe.isZero() or value.style != 0 or value.link != 0 or value.suffix.len != 0)) {
                return error.InvalidPadding;
            }
            if ((probe.wide == .spacer_tail) != pending_wide or
                (probe.wide == .wide and x + 1 >= d.cols) or
                (probe.wide == .spacer_head and (x + 1 != d.cols or meta[2] & 1 == 0))) return error.InvalidWideCell;
            pending_wide = probe.wide == .wide;
            for (0..value.suffix.len / 4) |i| {
                const cp = try cells.scalar(std.mem.readInt(u32, value.suffix[i * 4 ..][0..4], .little));
                if (@import("../shellfox_unicode_v6.zig").width(cp) != 0) return error.InvalidCell;
            }
            if (tally) |t| try t.text(@as(u64, @intFromBool(value.kind <= 1 and value.content != 0)) + value.suffix.len / 4, @as(u64, @intFromBool(value.kind <= 1 and value.content != 0)) * 4 + value.suffix.len);
            if (target) |p| try cells.materialize(value, p, p.getRow(y), &p.getCells(p.getRow(y))[x], if (value.style == 0) 0 else refs.styles[value.style], if (value.link == 0) 0 else refs.links[value.link]);
        }
    }
}
