const std = @import("std");
const testing = std.testing;
const envelope = @import("envelope.zig");
const record = @import("record.zig");
const guard = @import("../snapshot_budget_guard.zig");
const Crc32c = @import("../../crc32c.zig").Crc32c;

const limits: record.Limits = .{ .encoded_bytes = 4096, .records = 16, .pages = 2, .screens = 2 };
const totals: record.Totals = .{ .records = 7, .pages = 1, .physical_slots = 2, .text_scalars = 3 };
const Fixture = struct {
    bytes: [4096]u8,
    len: usize,
    finish_offset: usize,
};

// Opaque payloads deliberately test framing only, not a fake core/layout API.
fn fixture() !Fixture {
    var result: Fixture = undefined;
    var writer: std.Io.Writer = .fixed(&result.bytes);
    try writer.writeAll(&try envelope.encode(.{ .layout_abi = 7, .encoded_len = envelope.len }));
    try record.write(&writer, .compatibility, "metadata");
    try record.write(&writer, .terminal, "terminal");
    try record.write(&writer, .screen_layout, "screen");
    try record.write(&writer, .page_layout, "page");
    try record.write(&writer, .page_cells, "opaque");
    try record.write(&writer, .continuation, "");
    result.finish_offset = writer.end;
    try record.write(&writer, .finish, &totals.encode());
    result.len = writer.end;
    @memcpy(result.bytes[0..envelope.len], &try envelope.encode(.{ .layout_abi = 7, .encoded_len = result.len }));
    return result;
}

fn scope() guard.Scope {
    return .{ .max_bytes = 0, .max_work = 1000000 };
}

fn drain(reader: *record.Reader) !void {
    while (try reader.next()) |_| {}
}

fn repair(bytes: []u8, offset: usize) void {
    const header = bytes[offset..][0..record.header_len];
    const size = std.mem.readInt(u32, header[2..6], .little);
    var crc = Crc32c.init();
    crc.update(header[0..6]);
    crc.update(bytes[offset + record.header_len ..][0..size]);
    std.mem.writeInt(u32, header[6..10], crc.final(), .little);
}

test "format2 framing strict complete stream and measured FINISH" {
    const source = try fixture();
    var budget = scope();
    var reader = try record.Reader.init(source.bytes[0..source.len], 7, limits, &budget);
    try drain(&reader);
    try reader.verifyTotals(totals);
    try testing.expectEqual(source.len, reader.offset);
    try testing.expectEqual(totals.records, reader.records);
    try testing.expectEqual(@as(u64, 0), budget.heap_bytes + budget.mapped_bytes);
    try testing.expect(budget.work > envelope.len);
    var wrong = totals;
    wrong.physical_slots += 1;
    try testing.expectError(error.InvalidFinish, reader.verifyTotals(wrong));
    try testing.expectError(error.ReaderFailed, reader.next());
}

test "format2 framing rejects every record truncation without allocations" {
    const source = try fixture();
    for (envelope.len..source.len) |len| {
        var shortened = source;
        std.mem.writeInt(u64, shortened.bytes[24..32], len, .little);
        var budget = scope();
        var reader = try record.Reader.init(shortened.bytes[0..len], 7, limits, &budget);
        try testing.expectError(error.Truncated, drain(&reader));
        try testing.expectEqual(@as(u64, 0), budget.heap_bytes + budget.mapped_bytes);
    }
}

test "format2 framing rejects corruption unknown tags and invalid ordering" {
    const source = try fixture();
    var damaged = source;
    damaged.bytes[envelope.len + record.header_len] ^= 1;
    var budget = scope();
    var reader = try record.Reader.init(damaged.bytes[0..damaged.len], 7, limits, &budget);
    try testing.expectError(error.InvalidChecksum, reader.next());
    damaged = source;
    std.mem.writeInt(u16, damaged.bytes[envelope.len..][0..2], 65535, .little);
    budget = scope();
    reader = try record.Reader.init(damaged.bytes[0..damaged.len], 7, limits, &budget);
    try testing.expectError(error.UnsupportedRecord, reader.next());
    damaged = source;
    std.mem.writeInt(u16, damaged.bytes[envelope.len..][0..2], @backingInt(record.Tag.terminal), .little);
    repair(damaged.bytes[0..damaged.len], envelope.len);
    budget = scope();
    reader = try record.Reader.init(damaged.bytes[0..damaged.len], 7, limits, &budget);
    try testing.expectError(error.InvalidOrder, reader.next());
}

test "format2 framing refuses trailing bytes and false FINISH counts" {
    const source = try fixture();
    var damaged = source;
    damaged.bytes[damaged.len] = 0;
    damaged.len += 1;
    std.mem.writeInt(u64, damaged.bytes[24..32], damaged.len, .little);
    var budget = scope();
    var reader = try record.Reader.init(damaged.bytes[0..damaged.len], 7, limits, &budget);
    try testing.expectError(error.TrailingBytes, drain(&reader));
    damaged = source;
    damaged.bytes[damaged.finish_offset + record.header_len] ^= 1;
    repair(damaged.bytes[0..damaged.len], damaged.finish_offset);
    budget = scope();
    reader = try record.Reader.init(damaged.bytes[0..damaged.len], 7, limits, &budget);
    try testing.expectError(error.InvalidFinish, drain(&reader));
}

test "format2 framing deterministic work and count refusal" {
    const source = try fixture();
    var budget = scope();
    budget.max_work = 0;
    try testing.expectError(error.OutOfMemory, record.Reader.init(source.bytes[0..source.len], 7, limits, &budget));
    try testing.expect(budget.exceeded);
    budget = scope();
    budget.max_work = envelope.len + record.header_len + 2 * 8 - 1;
    var reader = try record.Reader.init(source.bytes[0..source.len], 7, limits, &budget);
    try testing.expectError(error.OutOfMemory, reader.next());
    try testing.expect(budget.exceeded);
    inline for (.{ "records", "pages", "screens" }) |key| {
        var reduced = limits;
        @field(reduced, key) = 0;
        budget = scope();
        reader = try record.Reader.init(source.bytes[0..source.len], 7, reduced, &budget);
        try testing.expectError(error.LimitExceeded, drain(&reader));
        try testing.expectEqual(@as(u64, 0), budget.heap_bytes + budget.mapped_bytes);
    }
}
