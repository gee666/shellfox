const std = @import("std");
const Terminal = @import("../Terminal.zig");
const Screen = @import("../Screen.zig");
const PageList = @import("../PageList.zig");
const Key = @import("../ScreenSet.zig").Key;
const layout = @import("../shellfox_cell_layout2.zig");
const meta = @import("../snapshot/screen.zig");
const wire = @import("../snapshot/io.zig");
const links = @import("../snapshot/hyperlink.zig");
const tables = @import("tables.zig");
const guard = @import("../snapshot_budget_guard.zig");
const Options = @import("options.zig").Options;

pub const View = struct {
    key: Key,
    layout: layout.ScreenLayout,
    pages: u32,
    header: meta.Header,
    saved: ?meta.SavedCursor,
    link: ?@import("../hyperlink.zig").Hyperlink,
    seen: bool,
    next_image: u32,
    next_placement: u32,
};
fn putCursor(w: *std.Io.Writer, c: layout.CursorLayout) !void {
    try wire.writeInt(w, u16, c.physical_x);
    try wire.writeInt(w, u16, c.y);
    try w.writeByte(@intFromBool(c.pending_wrap));
    try w.writeByte(c.virtual_overflow);
    try wire.writeInt(w, u16, 0);
}
fn getCursor(r: *std.Io.Reader, cols: u16, rows: u16) !layout.CursorLayout {
    const x = try wire.readInt(r, u16);
    const y = try wire.readInt(r, u16);
    const pending = try r.takeByte();
    const overflow = try r.takeByte();
    if (try wire.readInt(r, u16) != 0 or pending > 1 or overflow > 1 or x >= cols or y >= rows or
        (overflow == 1 and (pending != 1 or x != cols - 1))) return error.InvalidCursor;
    return .{ .physical_x = x, .y = y, .pending_wrap = pending == 1, .virtual_overflow = @intCast(overflow) };
}
fn header(s: *const Screen, key: Key, pages: u32) !meta.Header {
    var keyboard: meta.KittyKeyboard = .{ .index = s.kitty_keyboard.idx };
    for (s.kitty_keyboard.flags, &keyboard.flags) |from, *to| to.* = .{
        .disambiguate = from.disambiguate,
        .report_events = from.report_events,
        .report_alternates = from.report_alternates,
        .report_all = from.report_all,
        .report_associated = from.report_associated,
    };
    return .{
        .key = key,
        .page_count = std.math.cast(u16, pages) orelse return error.LimitExceeded,
        .history_rows = s.pages.total_rows - s.pages.rows,
        .cursor_x = s.cursor.x,
        .cursor_y = s.cursor.y,
        .cursor_style = s.cursor.cursor_style,
        .cursor_flags = .{ .pending_wrap = s.cursor.pending_wrap, .protected = s.cursor.protected, .semantic_content = s.cursor.semantic_content, .semantic_content_clear_eol = s.cursor.semantic_content_clear_eol },
        .cursor_pen = s.cursor.style,
        .hyperlink_implicit_id = s.cursor.hyperlink_implicit_id,
        .charset = s.charset,
        .protected_mode = s.protected_mode,
        .kitty_keyboard = keyboard,
        .semantic_click = s.semantic_prompt.click,
        .saved_cursor_present = s.saved_cursor != null,
    };
}
pub fn encode(w: *std.Io.Writer, t: *const Terminal, key: Key, pages: u32) !void {
    const s = t.screens.get(key) orelse return error.InvalidScreen;
    const state = (try layout.screenLayout(t, key)) orelse return error.InvalidScreen;
    try wire.writeInt(w, u16, @intCast(@backingInt(key)));
    try wire.writeInt(w, u16, state.visible_cols);
    try wire.writeInt(w, u16, state.physical_cols);
    try wire.writeInt(w, u16, state.rows);
    try wire.writeInt(w, u64, state.total_rows);
    try wire.writeInt(w, u32, pages);
    try putCursor(w, state.cursor);
    try w.writeByte(@intFromBool(state.saved_cursor != null));
    if (state.saved_cursor) |saved| try putCursor(w, saved);
    try (try header(s, key, pages)).encode(w);
    if (s.saved_cursor) |saved| try (meta.SavedCursor{
        .x = saved.x,
        .y = saved.y,
        .pen = saved.style,
        .flags = .{ .protected = saved.protected, .pending_wrap = saved.pending_wrap, .origin = saved.origin },
        .charset = saved.charset,
    }).encode(w);
    try meta.encodeCursorHyperlink(if (s.cursor.hyperlink) |link| link.* else null, w);
    try w.writeByte(@intFromBool(s.semantic_prompt.seen));
    if (comptime @import("terminal_options").kitty_graphics) {
        try wire.writeInt(w, u32, s.kitty_images.next_image_id);
        try wire.writeInt(w, u32, s.kitty_images.next_internal_placement_id);
    } else {
        try wire.writeInt(w, u32, 0);
        try wire.writeInt(w, u32, 0);
    }
}
pub fn parse(bytes: []const u8, o: Options, tally: ?*@import("resources.zig").Tally) !View {
    var r: std.Io.Reader = .fixed(bytes);
    const key = std.enums.fromInt(Key, std.math.cast(@typeInfo(Key).@"enum".tag_type, try wire.readInt(&r, u16)) orelse return error.InvalidScreen) orelse return error.InvalidScreen;
    const visible = try wire.readInt(&r, u16);
    const physical = try wire.readInt(&r, u16);
    const rows = try wire.readInt(&r, u16);
    const total_rows = try wire.readInt(&r, u64);
    const pages = try wire.readInt(&r, u32);
    if (visible < 2 or physical < visible or visible > o.max_visible_columns or physical > o.max_backing_columns or
        rows == 0 or rows > o.max_visible_rows or total_rows < rows or total_rows > o.max_rows or pages == 0 or pages > o.max_pages)
        return error.LimitExceeded;
    const current = try getCursor(&r, visible, rows);
    const present = try r.takeByte();
    if (present > 1) return error.InvalidCursor;
    const saved_layout = if (present == 1) try getCursor(&r, visible, rows) else null;
    const start = r.seek;
    const h = try meta.Header.decode(&r);
    var canonical: [meta.Header.len]u8 = undefined;
    var writer: std.Io.Writer = .fixed(&canonical);
    try h.encode(&writer);
    if (!std.mem.eql(u8, &canonical, bytes[start..r.seek]) or h.key != key or h.page_count != pages or
        h.history_rows != total_rows - rows or h.cursor_x != current.physical_x or h.cursor_y != current.y or
        h.cursor_flags.pending_wrap != current.pending_wrap or h.saved_cursor_present != (present == 1)) return error.InvalidScreen;
    const saved = if (h.saved_cursor_present) blk: {
        const saved_start = r.seek;
        const value = try meta.SavedCursor.decode(&r);
        var buf: [meta.SavedCursor.len]u8 = undefined;
        var sw: std.Io.Writer = .fixed(&buf);
        try value.encode(&sw);
        if (!std.mem.eql(u8, &buf, bytes[saved_start..r.seek]) or value.x != saved_layout.?.physical_x or
            value.y != saved_layout.?.y or value.flags.pending_wrap != saved_layout.?.pending_wrap) return error.InvalidCursor;
        break :blk value;
    } else null;
    const link_kind = try r.peekByte();
    const link = if (link_kind == 0) blk: {
        r.toss(1);
        break :blk null;
    } else try tables.readLink(&r);
    if (tally) |t| if (link) |l| try t.text(0, l.uri.len + if (l.id == .explicit) l.id.explicit.len else @as(usize, 0));
    const seen = try r.takeByte();
    if (seen > 1) return error.InvalidScreen;
    const next_image = try wire.readInt(&r, u32);
    const next_placement = try wire.readInt(&r, u32);
    if (r.bufferedLen() != 0) return error.InvalidScreen;
    return .{ .key = key, .layout = .{ .visible_cols = visible, .physical_cols = physical, .rows = rows, .total_rows = std.math.cast(usize, total_rows) orelse return error.LimitExceeded, .cursor = current, .saved_cursor = saved_layout }, .pages = pages, .header = h, .saved = saved, .link = link, .seen = seen == 1, .next_image = next_image, .next_placement = next_placement };
}
pub fn attach(t: *Terminal, view: View, pages: PageList) !void {
    const alloc = guard.heapAllocator();
    var owned_pages = pages;
    var transferred = false;
    errdefer if (!transferred) owned_pages.deinit();
    const c = view.layout.cursor;
    const pin = try owned_pages.trackPin(owned_pages.pin(.{ .active = .{ .x = c.physical_x, .y = c.y } }) orelse return error.InvalidCursor);
    const rc = pin.rowAndCell();
    const h = view.header;
    var result: Screen = .{
        .io = @import("../lib.zig").TinyIo.init.io(),
        .alloc = alloc,
        .pages = owned_pages,
        .no_scrollback = view.key == .alternate or t.screens.get(.primary).?.no_scrollback,
        .cursor = .{ .x = c.physical_x, .y = c.y, .cursor_style = h.cursor_style, .protected = h.cursor_flags.protected, .style = h.cursor_pen, .hyperlink_implicit_id = h.hyperlink_implicit_id, .semantic_content = h.cursor_flags.semantic_content, .semantic_content_clear_eol = h.cursor_flags.semantic_content_clear_eol, .page_pin = pin, .page_row = rc.row, .page_cell = rc.cell },
        .saved_cursor = if (view.saved) |s| .{ .x = s.x, .y = s.y, .style = s.pen, .protected = s.flags.protected, .pending_wrap = s.flags.pending_wrap, .origin = s.flags.origin, .charset = s.charset } else null,
        .charset = h.charset,
        .protected_mode = h.protected_mode,
        .semantic_prompt = .{ .seen = view.seen, .click = h.semantic_click },
    };
    result.kitty_keyboard.idx = @intCast(h.kitty_keyboard.index);
    for (h.kitty_keyboard.flags, &result.kitty_keyboard.flags) |from, *to| to.* = .{
        .disambiguate = from.disambiguate,
        .report_events = from.report_events,
        .report_alternates = from.report_alternates,
        .report_all = from.report_all,
        .report_associated = from.report_associated,
    };
    const slot = try t.screens.getInit(result.io, alloc, view.key, .{ .cols = view.layout.physical_cols, .rows = view.layout.rows, .kitty_image_storage_limit = 0 });
    slot.deinit();
    slot.* = result;
    transferred = true;
    if (comptime @import("terminal_options").kitty_graphics) {
        slot.kitty_images.setLimit(slot.io, alloc, slot, 0);
        slot.kitty_images.next_image_id = view.next_image;
        slot.kitty_images.next_internal_placement_id = view.next_placement;
    }
    try layout.restoreScreenLayoutBeforeContinuation(t, view.key, &view.layout);
}
pub fn restoreWork(capacity: @import("../page.zig").Capacity, view: View) !void {
    const slots = std.math.mul(u64, capacity.cols, capacity.rows) catch return error.LimitExceeded;
    const bytes: u64 = if (view.link) |l| l.uri.len + if (l.id == .explicit) l.id.explicit.len else @as(usize, 0) else 0;
    // A full cursor table can grow/split and copy managed cells. Bound all
    // capacity slots, numeric map probes and variable link comparisons.
    try @import("work.zig").native(@import("../page.zig").Page.layout(capacity).total_size, std.math.add(u64, slots, 1) catch return error.LimitExceeded, std.math.add(u64, bytes, 17) catch return error.LimitExceeded);
}
pub fn finish(s: *Screen, view: View) !void {
    try restoreWork(s.cursor.page_pin.node.pageAssumeResident().capacity, view);
    try s.manualStyleUpdate();
    if (view.link) |value| {
        if (value.id == .implicit) s.cursor.hyperlink_implicit_id = value.id.implicit;
        try s.startHyperlink(value.uri, if (value.id == .explicit) value.id.explicit else null);
        s.cursor.hyperlink_implicit_id = view.header.hyperlink_implicit_id;
    }
}
