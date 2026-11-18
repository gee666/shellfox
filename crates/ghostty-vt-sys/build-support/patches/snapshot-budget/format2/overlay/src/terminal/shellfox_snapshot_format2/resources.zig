//! Count admission only. Canonical layout validation remains Cora-owned.
const std = @import("std");
const guard = @import("../snapshot_budget_guard.zig");

pub const Limits = struct {
    pages: u64,
    rows: u64,
    visible_columns: u64,
    backing_columns: u64,
    physical_slots: u64,
    text_scalars: u64,
    text_bytes: u64,
};

pub const Tally = struct {
    limits: Limits,
    scope: *guard.Scope,
    pages: u64 = 0,
    rows: u64 = 0,
    physical_slots: u64 = 0,
    text_scalars: u64 = 0,
    text_bytes: u64 = 0,

    /// Admit actual physical stride, not visible width or sum of row lengths.
    /// The core must independently validate which geometry is legal.
    pub fn page(self: *Tally, visible_columns: u64, physical_columns: u64, rows: u64) !void {
        if (visible_columns == 0 or physical_columns == 0 or rows == 0) return error.InvalidDimensions;
        if (visible_columns > physical_columns) return error.InvalidDimensions;
        if (visible_columns > self.limits.visible_columns or physical_columns > self.limits.backing_columns)
            return error.LimitExceeded;
        const slots = std.math.mul(u64, physical_columns, rows) catch return error.LimitExceeded;
        // Native per-page refcounts remain u16, including temporary/cursor refs.
        if (slots > guard.max_page_cells) return error.LimitExceeded;
        const pages_next = std.math.add(u64, self.pages, 1) catch return error.LimitExceeded;
        const rows_next = std.math.add(u64, self.rows, rows) catch return error.LimitExceeded;
        const slots_next = std.math.add(u64, self.physical_slots, slots) catch return error.LimitExceeded;
        if (pages_next > self.limits.pages or rows_next > self.limits.rows or slots_next > self.limits.physical_slots)
            return error.LimitExceeded;
        const work = std.math.add(u64, slots, rows) catch return error.LimitExceeded;
        try self.scope.chargeWork(work);
        self.pages = pages_next;
        self.rows = rows_next;
        self.physical_slots = slots_next;
    }

    /// Check explicit resolved row backing length. Padding is still physical
    /// storage already charged by page(), even when outside this row length.
    pub fn row(self: *Tally, visible_columns: u64, physical_columns: u64, backing_columns: u64) !void {
        if (visible_columns == 0 or backing_columns < visible_columns or backing_columns > physical_columns)
            return error.InvalidRowLength;
        try self.scope.chargeWork(1);
    }

    /// Includes owned text from hidden cells and every managed table/string.
    /// Scalars/UTF encoding/reference validity are checked by the real codec.
    pub fn text(self: *Tally, scalars: u64, bytes: u64) !void {
        const scalars_next = std.math.add(u64, self.text_scalars, scalars) catch return error.LimitExceeded;
        const bytes_next = std.math.add(u64, self.text_bytes, bytes) catch return error.LimitExceeded;
        if (scalars_next > self.limits.text_scalars or bytes_next > self.limits.text_bytes)
            return error.LimitExceeded;
        const work = std.math.add(u64, scalars, bytes) catch return error.LimitExceeded;
        try self.scope.chargeWork(work);
        self.text_scalars = scalars_next;
        self.text_bytes = bytes_next;
    }
};

fn testTally(scope: *guard.Scope) Tally {
    return .{
        .scope = scope,
        .limits = .{ .pages = 2, .rows = 6, .visible_columns = 8, .backing_columns = 16, .physical_slots = 48, .text_scalars = 16, .text_bytes = 64 },
    };
}

test "format2 resources charge hidden stride and ragged row padding" {
    var scope: guard.Scope = .{ .max_bytes = 0, .max_work = 1000 };
    var tally = testTally(&scope);
    try tally.page(4, 8, 3);
    try tally.row(4, 8, 8);
    try tally.row(4, 8, 4);
    try tally.row(4, 8, 6);
    try std.testing.expectEqual(@as(u64, 24), tally.physical_slots);
    try std.testing.expectEqual(@as(u64, 30), scope.work);
    try tally.text(2, 8);
    try std.testing.expectEqual(@as(u64, 2), tally.text_scalars);
    try std.testing.expectEqual(@as(u64, 8), tally.text_bytes);
    try std.testing.expectEqual(@as(u64, 0), scope.heap_bytes + scope.mapped_bytes);
    tally.limits.physical_slots = 24;
    try std.testing.expectError(error.LimitExceeded, tally.page(4, 8, 1));
    try std.testing.expectEqual(@as(u64, 24), tally.physical_slots);
}

test "format2 resources reject hidden amplification invalid rows and overflow" {
    var scope: guard.Scope = .{ .max_bytes = 0, .max_work = 1000 };
    var tally = testTally(&scope);
    try std.testing.expectError(error.LimitExceeded, tally.page(1, 16, 4));
    try std.testing.expectEqual(@as(u64, 0), tally.pages);
    try std.testing.expectEqual(@as(u64, 0), scope.work);
    try std.testing.expectError(error.InvalidRowLength, tally.row(4, 8, 3));
    try std.testing.expectError(error.InvalidRowLength, tally.row(4, 8, 9));
    tally.limits.backing_columns = std.math.maxInt(u64);
    try std.testing.expectError(error.LimitExceeded, tally.page(1, std.math.maxInt(u64), 2));
    try std.testing.expectError(error.LimitExceeded, tally.text(std.math.maxInt(u64), 1));
    try tally.text(16, 64);
    try std.testing.expectError(error.LimitExceeded, tally.text(1, 0));
    try std.testing.expectError(error.LimitExceeded, tally.text(0, 1));
}

test "format2 resources deterministic work refusal is nonallocating" {
    var scope: guard.Scope = .{ .max_bytes = 0, .max_work = 26 };
    var tally = testTally(&scope);
    try std.testing.expectError(error.OutOfMemory, tally.page(4, 8, 3));
    try std.testing.expect(scope.exceeded);
    try std.testing.expectEqual(@as(u64, 0), tally.pages);
    try std.testing.expectEqual(@as(u64, 0), tally.physical_slots);
    try std.testing.expectEqual(@as(u64, 0), scope.heap_bytes + scope.mapped_bytes);
}
