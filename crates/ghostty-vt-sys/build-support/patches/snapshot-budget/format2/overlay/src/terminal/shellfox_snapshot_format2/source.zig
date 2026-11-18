//! Resident-only source admission. Must precede every profile/layout accessor.
const std = @import("std");
const Terminal = @import("../Terminal.zig");
const Key = @import("../ScreenSet.zig").Key;
const guard = @import("../snapshot_budget_guard.zig");
const Options = @import("options.zig").Options;
const work = @import("work.zig");

pub fn requireResident(t: *const Terminal, o: Options) !void {
    var count: u64 = 0;
    // Do not call node.page(), any profile/layout validator or content getter.
    for ([_]Key{ .primary, .alternate }) |key| if (t.screens.get(key)) |s| {
        var node = s.pages.pages.first;
        while (node) |current| : (node = current.next) {
            try guard.chargeWork(1);
            count = std.math.add(u64, count, 1) catch return error.LimitExceeded;
            if (count > o.max_pages) return error.LimitExceeded;
            if (current.pageIfResident() == null) return error.UnsupportedProfile;
        }
    };
}
/// ALL nodes already proved resident. Prepay the native validators' numeric
/// grapheme/map lookups and bounded suffix traversal before invoking them.
pub fn validationWork(t: *const Terminal) !void {
    for ([_]Key{ .primary, .alternate }) |key| if (t.screens.get(key)) |s| {
        var node = s.pages.pages.first;
        while (node) |current| : (node = current.next) {
            const p = current.pageIfResident() orelse return error.UnsupportedProfile;
            const slots = std.math.mul(u64, p.size.cols, p.size.rows) catch return error.LimitExceeded;
            try work.native(p.memory.len, slots, 24 + (@import("../page.zig").grapheme_max_len + 1) * 4);
        }
    };
}
