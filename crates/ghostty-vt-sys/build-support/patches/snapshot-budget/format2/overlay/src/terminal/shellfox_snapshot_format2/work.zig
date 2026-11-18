//! Conservative prepayment for native capacity-dependent managed operations.
const std = @import("std");
const guard = @import("../snapshot_budget_guard.zig");
const Page = @import("../page.zig").Page;

pub fn product(capacity: u64, operations: u64, comparison_bytes: u64) !u64 {
    return std.math.mul(u64, std.math.mul(u64, capacity, operations) catch return error.LimitExceeded, comparison_bytes) catch return error.LimitExceeded;
}
pub fn native(capacity: u64, operations: u64, comparison_bytes: u64) !void {
    try guard.chargeWork(try product(capacity, operations, comparison_bytes));
}
/// Page memory bounds every native table/bitmap capacity in that page. Numeric
/// map keys are at most8 bytes. Appending can scan/copy the bounded old suffix.
pub fn sourceLookups(p: *const Page, operations: u64) !void {
    try native(p.memory.len, operations, 8);
}
pub fn managed(capacity: u64, suffix_count: u64, has_link: bool) !void {
    try native(capacity, suffix_count, 8 + (@import("../page.zig").grapheme_max_len + 1) * 4);
    if (has_link) try native(capacity, 1, 8);
}

test "format2 work capacity products are checked and prepaid" {
    const testing = std.testing;
    try testing.expectEqual(@as(u64, 480), try product(20, 3, 8));
    try testing.expectError(error.LimitExceeded, product(std.math.maxInt(u64), 2, 1));
    var scope: guard.Scope = .{ .max_bytes = 0, .max_work = 479 };
    try testing.expect(guard.enter(&scope));
    defer guard.leave();
    try testing.expectError(error.OutOfMemory, native(20, 3, 8));
    try testing.expect(scope.exceeded);
    try testing.expectEqual(@as(u64, 0), scope.work);
}
