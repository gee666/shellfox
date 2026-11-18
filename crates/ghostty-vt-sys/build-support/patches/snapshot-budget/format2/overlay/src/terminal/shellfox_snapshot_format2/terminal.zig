//! Typed terminal metadata only. Never invokes the raw-v1 cell decoder.
const std = @import("std");
const Terminal = @import("../Terminal.zig");
const native = @import("../snapshot/terminal.zig");
const raw_record = @import("../snapshot/record.zig");
const colors = @import("../color.zig");
const wire = @import("../snapshot/io.zig");
const guard = @import("../snapshot_budget_guard.zig");
const Options = @import("options.zig").Options;

pub const View = struct {
    header: native.Header,
    tabstops: []const u8,
    original: []const u8,
    mask: []const u8,
    overrides: []const u8,
    pwd: []const u8,
    title: []const u8,
};
pub fn encode(w: *std.Io.Writer, t: *const Terminal) !void {
    // Reuse the unchanged metadata serializer, strip its record framing.
    var scratch: std.Io.Writer.Allocating = .init(guard.heapAllocator());
    defer scratch.deinit();
    var records: raw_record.Writer = .init(guard.heapAllocator(), &scratch.writer);
    defer records.deinit();
    try native.encode(t, &records);
    if (scratch.written().len < raw_record.Header.len) return error.InvalidTerminal;
    try w.writeAll(scratch.written()[raw_record.Header.len..]);
}
pub fn parse(bytes: []const u8, o: Options, tally: ?*@import("resources.zig").Tally) !View {
    var r: std.Io.Reader = .fixed(bytes);
    const header = try native.Header.decode(&r);
    var canonical: [native.Header.len]u8 = undefined;
    var writer: std.Io.Writer = .fixed(&canonical);
    try header.encode(&writer);
    if (bytes.len < canonical.len or !std.mem.eql(u8, &canonical, bytes[0..canonical.len])) return error.InvalidTerminal;
    if (header.columns < 2 or header.columns > o.max_visible_columns or header.rows == 0 or
        header.rows > o.max_visible_rows or header.width_px > o.max_pixel_width or header.height_px > o.max_pixel_height)
        return error.LimitExceeded;
    if (header.default_modes.grapheme_cluster or header.current_modes.grapheme_cluster or header.saved_modes.grapheme_cluster or
        header.status_display != .main) return error.UnsupportedProfile;
    const tabstops = try r.take((@as(usize, header.columns) + 7) / 8);
    if (header.columns % 8 != 0 and tabstops[tabstops.len - 1] >> @as(u3, @intCast(header.columns % 8)) != 0)
        return error.InvalidTerminal;
    const original = try r.take(256 * 3);
    const mask = try r.take(32);
    var count: usize = 0;
    for (mask) |byte| count += @popCount(byte);
    const overrides = try r.take(count * 3);
    const pwd = try r.take(try wire.readInt(&r, u32));
    const title = try r.take(try wire.readInt(&r, u32));
    if (r.bufferedLen() != 0) return error.InvalidTerminal;
    if (tally) |t| try t.text(0, pwd.len + title.len);
    return .{ .header = header, .tabstops = tabstops, .original = original, .mask = mask, .overrides = overrides, .pwd = pwd, .title = title };
}
fn rgb(bytes: []const u8) colors.RGB {
    return .{ .r = bytes[0], .g = bytes[1], .b = bytes[2] };
}
pub fn fresh(view: View) !@import("../shellfox_cell_layout2.zig").FreshRestore {
    const h = view.header;
    const alloc = guard.heapAllocator();
    var original: colors.Palette = undefined;
    for (&original, 0..) |*value, i| value.* = rgb(view.original[i * 3 ..][0..3]);
    var palette = try colors.DynamicPalette.init(alloc, original);
    errdefer palette.deinit(alloc);
    var j: usize = 0;
    for (0..256) |i| if (view.mask[i / 8] & (@as(u8, 1) << @as(u3, @intCast(i % 8))) != 0) {
        palette.set(@intCast(i), rgb(view.overrides[j..][0..3]));
        j += 3;
    };
    var result = try @import("../shellfox_cell_layout2.zig").newFreshRestore(.{
        .cols = h.columns,
        .rows = h.rows,
        .max_scrollback_bytes = if (h.max_scrollback_bytes) |n| std.math.cast(usize, n) orelse return error.LimitExceeded else null,
        .max_scrollback_lines = if (h.max_scrollback_rows) |n| std.math.cast(usize, n) orelse return error.LimitExceeded else null,
        .colors = .{ .background = h.background, .foreground = h.foreground, .cursor = h.cursor_color, .palette = palette },
        .default_modes = h.default_modes,
        .default_cursor_style = h.cursor_default_style,
        .default_cursor_blink = h.cursor_default_blink,
        .kitty_image_storage_limit = 0,
    });
    // Ownership of palette was transferred by the actual fresh factory.
    palette = .default;
    errdefer result.deinit();
    const t = try result.terminal();
    t.width_px = h.width_px;
    t.height_px = h.height_px;
    t.scrolling_region = .{ .top = h.scrolling_region_top, .bottom = h.scrolling_region_bottom, .left = h.scrolling_region_left, .right = h.scrolling_region_right };
    t.previous_char = h.previous_codepoint;
    t.cursor = .{ .is_default = h.cursor_is_default, .default_style = h.cursor_default_style, .default_blink = h.cursor_default_blink };
    t.modes = .{ .values = h.current_modes, .saved = h.saved_modes, .default = h.default_modes };
    t.flags.shell_redraws_prompt = h.shell_redraw;
    t.flags.modify_other_keys_2 = h.modify_other_keys_2;
    t.flags.mouse_event = h.mouse_event;
    t.flags.mouse_format = h.mouse_format;
    t.flags.mouse_shift_capture = if (h.mouse_shift_capture) |v| if (v) .true else .false else .null;
    t.flags.password_input = h.password_input;
    t.mouse_shape = h.mouse_shape;
    try t.setPwd(view.pwd);
    try t.setTitle(view.title);
    var stops = try @import("../Tabstops.zig").init(alloc, h.columns, 0);
    errdefer stops.deinit(alloc);
    for (0..h.columns) |i| if (view.tabstops[i / 8] & (@as(u8, 1) << @as(u3, @intCast(i % 8))) != 0) stops.set(i);
    t.tabstops.deinit(alloc);
    t.tabstops = stops;
    return result;
}
