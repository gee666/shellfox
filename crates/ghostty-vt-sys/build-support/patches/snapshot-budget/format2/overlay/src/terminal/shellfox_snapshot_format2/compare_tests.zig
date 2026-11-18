const std = @import("std");
const testing = std.testing;
const ct = @import("../c/terminal.zig");
const layout = @import("../shellfox_cell_layout2.zig");
const profile = @import("../shellfox_terminal_profile.zig");
const NativePage = @import("../page.zig").Page;
const Cell = @import("../page.zig").Cell;
const Style = @import("../style.zig").Style;

fn cell(a: *const NativePage, ac: *const Cell, b: *const NativePage, bc: *const Cell) !void {
    try testing.expectEqual(try layout.cellLayout(ac.*, 1), try layout.cellLayout(bc.*, 1));
    try testing.expectEqual(ac.content_tag, bc.content_tag);
    try testing.expectEqual(ac.protected, bc.protected);
    try testing.expectEqual(ac.semantic_content, bc.semantic_content);
    switch (ac.content_tag) {
        .codepoint, .codepoint_grapheme => try testing.expectEqual(ac.codepoint(), bc.codepoint()),
        .bg_color_palette => try testing.expectEqual(ac.content.color_palette.data, bc.content.color_palette.data),
        .bg_color_rgb => try testing.expectEqual(ac.content.color_rgb, bc.content.color_rgb),
    }
    const as: Style = if (ac.style_id == 0) .{} else a.styles.get(a.memory, ac.style_id).*;
    const bs: Style = if (bc.style_id == 0) .{} else b.styles.get(b.memory, bc.style_id).*;
    try testing.expect(as.eql(bs));
    try testing.expectEqualSlices(u21, a.lookupGrapheme(ac) orelse &.{}, b.lookupGrapheme(bc) orelse &.{});
    const al = a.lookupHyperlink(ac);
    const bl = b.lookupHyperlink(bc);
    try testing.expectEqual(al == null, bl == null);
    if (al) |id| {
        const av = a.hyperlink_set.get(a.memory, id);
        const bv = b.hyperlink_set.get(b.memory, bl.?);
        try testing.expectEqualStrings(av.uri.slice(a.memory), bv.uri.slice(b.memory));
        try testing.expectEqual(std.meta.activeTag(av.id), std.meta.activeTag(bv.id));
        switch (av.id) {
            .implicit => |value| try testing.expectEqual(value, bv.id.implicit),
            .explicit => |value| try testing.expectEqualStrings(value.slice(a.memory), bv.id.explicit.slice(b.memory)),
        }
    }
}
pub fn terminals(first: ct.Terminal, second: ct.Terminal) !void {
    const a = ct.zigTerminal(first).?;
    const b = ct.zigTerminal(second).?;
    try testing.expectEqual(a.cols, b.cols);
    try testing.expectEqual(a.rows, b.rows);
    const ast = try profile.capture(a);
    const bst = try profile.capture(b);
    try testing.expectEqual(ast.terminal_profile, bst.terminal_profile);
    try testing.expectEqual(ast.preceding_join, bst.preceding_join);
    try testing.expectEqual(a.screens.active_key, b.screens.active_key);
    try testing.expectEqual(a.flags.shell_redraws_prompt, b.flags.shell_redraws_prompt);
    try testing.expectEqual(a.flags.resize_pull_scrollback, b.flags.resize_pull_scrollback);
    for ([_]@import("../ScreenSet.zig").Key{ .primary, .alternate }) |key| {
        const av = try layout.screenLayout(a, key);
        const bv = try layout.screenLayout(b, key);
        try testing.expectEqual(av, bv);
        const as = a.screens.get(key) orelse continue;
        const bs = b.screens.get(key).?;
        try testing.expectEqual(as.saved_cursor, bs.saved_cursor);
        try testing.expect(as.cursor.style.eql(bs.cursor.style));
        try testing.expectEqual(as.charset, bs.charset);
        var ai = as.pages.rowIterator(.right_down, .{ .screen = .{} }, null);
        var bi = bs.pages.rowIterator(.right_down, .{ .screen = .{} }, null);
        while (ai.next()) |ap| {
            const bp = bi.next() orelse return error.TestUnexpectedResult;
            const ar = ap.rowAndCell().row;
            const br = bp.rowAndCell().row;
            const ag = ap.node.pageAssumeResident();
            const bg = bp.node.pageAssumeResident();
            try testing.expectEqual(try layout.rowBackingCols(as, ag, ar), try layout.rowBackingCols(bs, bg, br));
            try testing.expectEqual(ar.wrap, br.wrap);
            try testing.expectEqual(ar.wrap_continuation, br.wrap_continuation);
            try testing.expectEqual(ar.semantic_prompt, br.semantic_prompt);
            const ac = ag.getCells(ar);
            const bc = bg.getCells(br);
            try testing.expectEqual(ac.len, bc.len);
            for (ac, bc) |*avc, *bvc| try cell(ag, avc, bg, bvc);
        }
        try testing.expect(bi.next() == null);
    }
    const at = try a.plainString(testing.allocator);
    defer testing.allocator.free(at);
    const bt = try b.plainString(testing.allocator);
    defer testing.allocator.free(bt);
    try testing.expectEqualStrings(at, bt);
}
