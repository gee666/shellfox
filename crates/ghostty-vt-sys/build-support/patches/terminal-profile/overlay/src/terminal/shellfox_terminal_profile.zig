//! Immutable parser compatibility profile and checked restoration state.
//! The attributed width/join table lives ONLY in shellfox_unicode_v6.zig.
const std = @import("std");
const Terminal = @import("Terminal.zig");
const build = @import("terminal_options");
const modes = @import("modes.zig");
const v6 = @import("shellfox_unicode_v6.zig");
pub const Native: u32 = 0;
pub const LegacyXterm6: u32 = 1;
pub const abi_version: u32 = 1;
// Native-only producer prerequisite checkpoint, not a final compatibility pin.
pub const legacy_live_enabled = false;
pub const legacy_cell_layout_version: u32 = 2;
pub const Error = error{ UnsupportedProfile, IncompatibleState, LimitExceeded };
/// Canonical public GhosttyShellfoxTerminalProfileState. Never serialize padding.
pub const State = extern struct {
    size: usize = @sizeOf(State),
    abi_version: u32 = 1,
    terminal_profile: u32,
    preceding_join: u32,
};
pub fn validateState(state: *const State, requested: u32) Error!void {
    if (requested != Native and requested != LegacyXterm6) return error.UnsupportedProfile;
    if (state.size != @sizeOf(State) or state.abi_version != abi_version or state.terminal_profile != requested)
        return error.IncompatibleState;
    if (requested == Native) {
        if (state.preceding_join != 0) return error.IncompatibleState;
    } else if (!v6.validState(state.preceding_join)) return error.IncompatibleState;
}
pub fn capture(t: *const Terminal) Error!State {
    const state: State = .{ .terminal_profile = t.shellfox_terminal_profile, .preceding_join = t.shellfox_preceding_join };
    try validateState(&state, state.terminal_profile);
    if (t.shellfox_profile_incompatible or (state.terminal_profile == LegacyXterm6 and !t.shellfox_profile_state_known))
        return error.IncompatibleState;
    return state;
}
/// Internal constructor hook, called on a freshly allocated terminal before feed.
/// No public setter exists. Native construction retains every upstream default.
pub fn initialize(t: *Terminal, requested: u32) Error!void {
    if (requested != Native and requested != LegacyXterm6) return error.UnsupportedProfile;
    if (requested == LegacyXterm6) {
        t.modes.setDefault(.grapheme_cluster, false);
        t.setKittyGraphicsSizeLimit(t.gpa(), 0);
        t.setKittyGraphicsLoadingLimits(.direct);
    }
    t.shellfox_terminal_profile = requested;
    t.shellfox_preceding_join = 0;
    t.shellfox_profile_state_known = true;
}
/// Validate an ALREADY installed captured profile state. Missing Legacy state is
/// never synthesized from cells, modes, a build hash, or a constructor default.
pub fn applyRestored(t: *Terminal, requested: u32, max_cells: usize) Error!void {
    if (requested == LegacyXterm6 and !legacy_live_enabled) return error.UnsupportedProfile;
    const state = try capture(t);
    try validateState(&state, requested);
    try validateCells(t, requested, max_cells);
}
/// Fresh decoded terminal only, BEFORE continuation replay. Validate everything
/// before mutation. Missing Legacy State rejects. Native needs no legacy state.
/// Decoder must compare capture AFTER replay with the source cut State and must
/// not publish a partially decoded/validated artifact.
pub fn restoreStateBeforeContinuation(t: *Terminal, requested: u32, state: ?*const State, max_cells: usize) Error!void {
    if (requested != Native and requested != LegacyXterm6) return error.UnsupportedProfile;
    if (requested == LegacyXterm6 and !legacy_live_enabled) return error.UnsupportedProfile;
    const native: State = .{ .terminal_profile = Native, .preceding_join = 0 };
    const supplied = state orelse if (requested == Native) &native else return error.IncompatibleState;
    try validateState(supplied, requested);
    try validateCells(t, requested, max_cells);
    t.shellfox_terminal_profile = requested;
    t.shellfox_preceding_join = supplied.preceding_join;
    t.shellfox_profile_state_known = true;
}
fn validateCells(t: *const Terminal, requested: u32, max_cells: usize) Error!void {
    if (requested != Native and requested != LegacyXterm6) return error.UnsupportedProfile;
    if (t.shellfox_profile_incompatible) return error.IncompatibleState;
    var count: usize = 0;
    var all_screens = t.screens.all;
    var screens = all_screens.iterator();
    while (screens.next()) |entry| {
        count = std.math.add(usize, count, std.math.mul(usize, entry.value.*.pages.total_rows, t.cols) catch return error.LimitExceeded) catch return error.LimitExceeded;
        if (count > max_cells) return error.LimitExceeded;
    }
    if (requested == Native) return;
    const saved: modes.ModeState = .{ .values = t.modes.saved };
    if (t.modes.get(.grapheme_cluster) or saved.get(.grapheme_cluster) or t.modes.getDefault(.grapheme_cluster)) return error.IncompatibleState;
    if (t.glyph_glossary.entries.count() != 0 or t.kitty_dnd != null or t.status_display != .main) return error.IncompatibleState;
    screens = all_screens.iterator();
    while (screens.next()) |entry| {
        const screen = entry.value.*;
        if (screen.kitty_keyboard.idx != 0) return error.IncompatibleState;
        for (screen.kitty_keyboard.flags) |flags| if (flags.int() != 0) return error.IncompatibleState;
        if (comptime build.kitty_graphics) {
            if (screen.kitty_images.total_limit != 0 or screen.kitty_images.total_bytes != 0 or screen.kitty_images.images.count() != 0 or screen.kitty_images.placements.count() != 0 or screen.kitty_images.loading != null or !std.meta.eql(screen.kitty_images.image_limits, @import("kitty.zig").graphics.LoadingImage.Limits.direct)) return error.IncompatibleState;
        }
        var pages = screen.pages.getTopLeft(.screen).pageIterator(.right_down, screen.pages.getBottomRight(.screen).?);
        while (pages.next()) |chunk| {
            const page = chunk.node.page();
            for (chunk.start..chunk.end) |y| {
                const row = page.getRow(y);
                if (row.kitty_virtual_placeholder) return error.IncompatibleState;
                for (page.getCells(row)) |*cell| if (cell.hasText()) {
                    const width: u2 = if (cell.wide == .wide) 2 else 1;
                    if (v6.clusterWidth(cell.codepoint(), page.lookupGrapheme(cell) orelse &.{}) != width) return error.IncompatibleState;
                };
            }
        }
    }
}
/// Actual parsed dispatch/execute boundary, not feed-chunk boundaries.
pub fn boundary(t: *Terminal) void {
    if (t.shellfox_terminal_profile == LegacyXterm6) t.shellfox_preceding_join = 0;
}
pub fn reject(t: *Terminal) error{UnsupportedTerminalProfile} {
    t.shellfox_profile_incompatible = true;
    return error.UnsupportedTerminalProfile;
}
/// REP reads the prior cell using the EXACT preceding property, not previous_char.
/// Its CSI dispatch boundary resets the property only AFTER repetition completes.
pub fn repeat(t: *Terminal, requested: usize) !void {
    const state = t.shellfox_preceding_join;
    if (state == 0) return;
    const width: u16 = @intCast((state >> 1) & 3);
    const screen = t.screens.active;
    const virtual_x: usize = screen.cursor.x + @as(usize, @intFromBool(screen.cursor.pending_wrap));
    if (virtual_x < width) return reject(t);
    const left: u16 = width - @as(u16, @intFromBool(screen.cursor.pending_wrap));
    const cell = screen.cursorCellLeft(left);
    if (!cell.hasText()) return reject(t);
    var cps: [@import("page.zig").grapheme_max_len + 1]u21 = undefined;
    cps[0] = cell.codepoint();
    const extra = screen.cursor.page_pin.node.page().lookupGrapheme(cell) orelse &.{};
    @memcpy(cps[1..][0..extra.len], extra);
    for (0..@max(requested, 1)) |_| for (cps[0 .. extra.len + 1]) |cp| try t.print(cp);
}
test {
    _ = @import("shellfox_profile_unicode_tests.zig");
    _ = @import("shellfox_unicode_v6.zig");
    _ = @import("shellfox_terminal_profile_tests.zig");
}
