const std = @import("std");
const ct = @import("../c/terminal.zig");
const native_snapshot = @import("../snapshot/main.zig");
const continuation = @import("../snapshot/continuation.zig");
const guard = @import("../snapshot_budget_guard.zig");
const layout = @import("../shellfox_cell_layout2.zig");
const profile = @import("../shellfox_terminal_profile.zig");
const Key = @import("../ScreenSet.zig").Key;
const PageList = @import("../PageList.zig");
const options = @import("options.zig");
const envelope = @import("envelope.zig");
const record = @import("record.zig");
const resources = @import("resources.zig");
const term = @import("terminal.zig");
const screens = @import("screen.zig");
const pages = @import("page.zig");
pub const Options = options.Options;
pub const supported_build = @import("../c/shellfox_snapshot_budget.zig").supported_build;
pub fn enabled() bool {
    return supported_build and @import("../shellfox_layout2_gate.zig").enabled();
}

pub const Body = struct {
    allocation: []u8,
    len: usize,
    totals: record.Totals,
    pub fn bytes(self: Body) []const u8 {
        return self.allocation[0..self.len];
    }
    pub fn deinit(self: Body) void {
        guard.heapAllocator().free(self.allocation);
    }
};
const Admitted = struct {
    compatibility: options.Compatibility,
    terminal: term.View,
    screens: [2]screens.View,
    screen_count: usize,
    continuation: []const u8,
    totals: record.Totals,
};
fn frameLimits(o: Options) record.Limits {
    return .{ .encoded_bytes = o.max_encoded_bytes, .records = o.max_records, .pages = o.max_pages, .screens = 2 };
}
fn tally(o: Options, scope: *guard.Scope) resources.Tally {
    return .{ .scope = scope, .limits = .{ .pages = o.max_pages, .rows = o.max_rows, .visible_columns = o.max_visible_columns, .backing_columns = o.max_backing_columns, .physical_slots = o.max_physical_slots, .text_scalars = o.max_text_scalars, .text_bytes = o.max_text_bytes } };
}
fn admit(bytes: []const u8, o: Options, scope: *guard.Scope) !Admitted {
    var reader = try record.Reader.init(bytes, o.layout_abi, frameLimits(o), scope);
    var count = tally(o, scope);
    var result: Admitted = undefined;
    result.screen_count = 0;
    result.continuation = &.{};
    var rows: u64 = 0;
    var page_count: u32 = 0;
    var pending: ?pages.Description = null;
    var routed: u8 = 0;
    while (try reader.next()) |frame| switch (frame.tag) {
        .compatibility => result.compatibility = try options.decodeCompatibility(frame.payload, o),
        .terminal => result.terminal = try term.parse(frame.payload, o, &count),
        .screen_layout => {
            if (result.screen_count > 0 and (rows != result.screens[result.screen_count - 1].layout.total_rows or
                page_count != result.screens[result.screen_count - 1].pages)) return error.InvalidScreen;
            if (result.screen_count >= 2) return error.InvalidScreen;
            const view = try screens.parse(frame.payload, o, &count);
            const bit: u8 = @as(u8, 1) << @as(u3, @intCast(@backingInt(view.key)));
            if (routed & bit != 0 or view.layout.visible_cols != result.terminal.header.columns or
                view.layout.rows != result.terminal.header.rows or
                (view.key == .primary and view.layout.physical_cols != view.layout.visible_cols)) return error.InvalidScreen;
            routed |= bit;
            result.screens[result.screen_count] = view;
            result.screen_count += 1;
            rows = 0;
            page_count = 0;
        },
        .page_layout => {
            const view = result.screens[result.screen_count - 1];
            pending = try pages.describe(frame.payload, view.layout.visible_cols, &count);
            if (pending.?.cols != view.layout.physical_cols) return error.InvalidPage;
            const row_start = rows;
            rows = std.math.add(u64, rows, pending.?.rows) catch return error.LimitExceeded;
            const cursor_row = view.layout.total_rows - view.layout.rows + view.layout.cursor.y;
            if (cursor_row >= row_start and cursor_row < rows) try screens.restoreWork(pending.?.capacity, view);
            page_count += 1;
        },
        .page_cells => try pages.decodeCells(frame.payload, pending orelse return error.InvalidPage, null, null, &count),
        .continuation => {
            if (frame.payload.len > o.max_continuation_bytes) return error.LimitExceeded;
            try scope.chargeWork(std.math.mul(u64, frame.payload.len, frame.payload.len) catch return error.LimitExceeded);
            try continuation.validate(.{ .bytes = frame.payload });
            result.continuation = frame.payload;
        },
        .finish => {
            if (rows != result.screens[result.screen_count - 1].layout.total_rows or
                page_count != result.screens[result.screen_count - 1].pages or
                result.screen_count != result.terminal.header.screen_count or routed & 1 == 0 or
                routed & (@as(u8, 1) << @as(u3, @intCast(@backingInt(result.terminal.header.active_screen_key)))) == 0)
                return error.InvalidScreen;
            result.totals = .{ .records = reader.records, .pages = count.pages, .physical_slots = count.physical_slots, .text_scalars = count.text_scalars };
            try reader.verifyTotals(result.totals);
        },
    };
    if (reader.phase != .complete) return error.MissingFinish;
    return result;
}
fn emit(w: *std.Io.Writer, tag: record.Tag, payload: []const u8, records: *u64) !void {
    try record.write(w, tag, payload);
    records.* += 1;
}
pub const OutputError = error{ OutOfSpace, EncodedLimitExceeded, ContinuationLimitExceeded };
pub fn outputFailure(o: Options, capacity: usize) OutputError {
    return if (capacity < o.max_encoded_bytes) error.OutOfSpace else error.EncodedLimitExceeded;
}
pub fn encode(source: ct.Terminal, o: Options, scope: *guard.Scope, capacity: usize) !Body {
    return encodeStaged(source, o, scope, capacity) catch |err| {
        if (err == error.WriteFailed) {
            if (scope.failed()) return error.OutOfMemory;
            return outputFailure(o, capacity);
        }
        return err;
    };
}
fn encodeStaged(source: ct.Terminal, o: Options, scope: *guard.Scope, capacity: usize) !Body {
    if (!enabled() or !guard.isActive()) return error.UnsupportedProfile;
    try options.validate(o);
    const t = ct.zigTerminal(source) orelse return error.InvalidTerminal;
    try @import("source.zig").requireResident(t, o);
    try @import("source.zig").validationWork(t);
    if (!ct.budgetRawV1ConfigSupported(source) or t.flags.resize_pull_scrollback != (o.runtime_resize_pull_scrollback == 1))
        return error.UnsupportedProfile;
    const state = try profile.capture(t);
    try profile.validateState(&state, o.terminal_profile);
    try profile.applyRestored(t, state.terminal_profile, std.math.cast(usize, o.max_physical_slots) orelse return error.LimitExceeded);
    _ = try layout.validateLayout(t, state.terminal_profile, .{ .max_cells = std.math.cast(usize, o.max_physical_slots) orelse return error.LimitExceeded, .max_rows = std.math.cast(usize, o.max_rows) orelse return error.LimitExceeded });
    const alloc = guard.heapAllocator();
    const cap = @min(capacity, std.math.cast(usize, o.max_encoded_bytes) orelse return error.LimitExceeded);
    if (cap < envelope.len) return outputFailure(o, capacity);
    const stage = try alloc.alloc(u8, cap);
    errdefer alloc.free(stage);
    const scratch = try alloc.alloc(u8, cap);
    defer alloc.free(scratch);
    var output: std.Io.Writer = .fixed(stage);
    try output.writeAll(&try envelope.encode(.{ .layout_abi = layout.version, .encoded_len = envelope.len }));
    var records: u64 = 0;
    var count = tally(o, scope);
    var w: std.Io.Writer = .fixed(scratch);
    try options.encodeCompatibility(&w, o, state);
    try emit(&output, .compatibility, w.buffered(), &records);
    w = .fixed(scratch);
    try term.encode(&w, t);
    _ = try term.parse(w.buffered(), o, &count);
    try emit(&output, .terminal, w.buffered(), &records);
    for ([_]Key{ .primary, .alternate }) |key| if (t.screens.get(key)) |s| {
        var n = s.pages.pages.first;
        var pc: u32 = 0;
        while (n) |node| : (n = node.next) {
            if (node.pageIfResident() == null) return error.UnsupportedProfile;
            pc += 1;
            if (pc > o.max_pages) return error.LimitExceeded;
        }
        w = .fixed(scratch);
        try screens.encode(&w, t, key, pc);
        const view = try screens.parse(w.buffered(), o, &count);
        try emit(&output, .screen_layout, w.buffered(), &records);
        n = s.pages.pages.first;
        while (n) |node| : (n = node.next) {
            const p = node.pageIfResident() orelse return error.UnsupportedProfile;
            w = .fixed(scratch);
            try pages.encodeLayout(&w, s, p);
            _ = try pages.describe(w.buffered(), view.layout.visible_cols, &count);
            const payload_len = w.buffered().len;
            const layout_start = output.end + record.header_len;
            try emit(&output, .page_layout, w.buffered(), &records);
            const desc = try pages.describe(output.buffered()[layout_start..][0..payload_len], view.layout.visible_cols, null);
            w = .fixed(scratch);
            try pages.encodeCells(&w, p);
            try pages.decodeCells(w.buffered(), desc, null, null, &count);
            try emit(&output, .page_cells, w.buffered(), &records);
        }
    };
    w = .fixed(scratch[0..@min(scratch.len, std.math.cast(usize, o.max_continuation_bytes) orelse return error.LimitExceeded)]);
    ct.continuationWriteIo(source, &w) catch |err| switch (err) {
        error.WriteFailed => {
            if (scope.failed()) return error.OutOfMemory;
            if (o.max_continuation_bytes <= scratch.len) return error.ContinuationLimitExceeded;
            return outputFailure(o, capacity);
        },
        error.ContinuationDisabled => {
            const empty = try ct.continuationAllocIo(source, alloc, true);
            defer alloc.free(empty);
            if (empty.len != 0) return error.InvalidContinuation;
        },
        else => return err,
    };
    try emit(&output, .continuation, w.buffered(), &records);
    const totals: record.Totals = .{ .records = records + 1, .pages = count.pages, .physical_slots = count.physical_slots, .text_scalars = count.text_scalars };
    try emit(&output, .finish, &totals.encode(), &records);
    @memcpy(stage[0..envelope.len], &try envelope.encode(.{ .layout_abi = layout.version, .encoded_len = output.end }));
    const checked = try admit(stage[0..output.end], o, scope);
    const after = try profile.capture(t);
    if (after.terminal_profile != state.terminal_profile or after.preceding_join != state.preceding_join) return error.IncompatibleState;
    if (scope.failed()) return error.OutOfMemory;
    return .{ .allocation = stage, .len = output.end, .totals = checked.totals };
}

fn restoreScreen(bytes: []const u8, o: Options, scope: *guard.Scope, t: *ct.ZigTerminal, view: screens.View, meta: term.View) !void {
    const alloc = guard.heapAllocator();
    var builder = try PageList.Builder.init(alloc, .{ .cols = view.layout.physical_cols, .rows = view.layout.rows, .max_size = if (view.key == .alternate) 0 else if (meta.header.max_scrollback_bytes) |n| std.math.cast(usize, n) orelse return error.LimitExceeded else null, .max_lines = if (view.key == .alternate) 0 else if (meta.header.max_scrollback_rows) |n| std.math.cast(usize, n) orelse return error.LimitExceeded else null });
    defer builder.deinit();
    var reader = try record.Reader.init(bytes, o.layout_abi, frameLimits(o), scope);
    var matching = false;
    while (try reader.next()) |frame| switch (frame.tag) {
        .screen_layout => matching = (try screens.parse(frame.payload, o, null)).key == view.key,
        .page_layout => if (matching) {
            const d = try pages.describe(frame.payload, view.layout.visible_cols, null);
            const p = try builder.allocatePage(d.capacity);
            p.size = .{ .cols = d.cols, .rows = d.rows };
        },
        else => {},
    };
    try screens.attach(t, view, try builder.finish());
    const s = t.screens.get(view.key).?;
    var node = s.pages.pages.first;
    reader = try record.Reader.init(bytes, o.layout_abi, frameLimits(o), scope);
    var d: ?pages.Description = null;
    matching = false;
    while (try reader.next()) |frame| switch (frame.tag) {
        .screen_layout => matching = (try screens.parse(frame.payload, o, null)).key == view.key,
        .page_layout => if (matching) {
            d = try pages.describe(frame.payload, view.layout.visible_cols, null);
        },
        .page_cells => if (matching) {
            const current = node orelse return error.InvalidPage;
            try pages.decodeCells(frame.payload, d orelse return error.InvalidPage, s, current.pageAssumeResident(), null);
            node = current.next;
        },
        else => {},
    };
    if (node != null) return error.InvalidPage;
    try screens.finish(s, view);
}
pub const Restored = struct { terminal: ct.Terminal, totals: record.Totals };
pub fn decode(bytes: []const u8, o: Options, scope: *guard.Scope) !Restored {
    if (!enabled() or !guard.isActive()) return error.UnsupportedProfile;
    try options.validate(o);
    const checked = try admit(bytes, o, scope);
    var fresh = try term.fresh(checked.terminal);
    defer fresh.deinit();
    const t = try fresh.terminal();
    t.flags.resize_pull_scrollback = checked.compatibility.resize_pull;
    for (checked.screens[0..checked.screen_count]) |view| try restoreScreen(bytes, o, scope, t, view, checked.terminal);
    t.screens.switchTo(checked.terminal.header.active_screen_key);
    try fresh.installProfileState(&checked.compatibility.state, .{ .max_cells = std.math.cast(usize, o.max_physical_slots) orelse return error.LimitExceeded, .max_rows = std.math.cast(usize, o.max_rows) orelse return error.LimitExceeded });
    const alloc = guard.heapAllocator();
    const tail = try alloc.dupe(u8, checked.continuation);
    var tail_transferred = false;
    errdefer if (!tail_transferred) alloc.free(tail);
    var decoded: native_snapshot.Decoded = .{ .terminal = try fresh.takeForContinuation(), .continuation = if (tail.len == 0) .ground else .{ .bytes = tail }, .history_rows = .init(.{}) };
    tail_transferred = true;
    defer decoded.deinit(alloc);
    const result = try ct.fromDecoded(alloc, .init, &decoded, std.math.cast(usize, o.max_continuation_bytes) orelse return error.LimitExceeded);
    errdefer ct.free(result);
    const restored = ct.zigTerminal(result).?;
    const state = try profile.capture(restored);
    if (state.terminal_profile != checked.compatibility.state.terminal_profile or
        state.preceding_join != checked.compatibility.state.preceding_join or !ct.budgetRawV1ConfigSupported(result)) return error.IncompatibleState;
    _ = try layout.validateLayout(restored, state.terminal_profile, .{ .max_cells = std.math.cast(usize, o.max_physical_slots) orelse return error.LimitExceeded, .max_rows = std.math.cast(usize, o.max_rows) orelse return error.LimitExceeded });
    if (scope.failed()) return error.OutOfMemory;
    return .{ .terminal = result, .totals = checked.totals };
}
