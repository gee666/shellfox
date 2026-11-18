const std = @import("std");
const lib = @import("../lib.zig");
const identity = @import("../shellfox_profile.zig");
const layout = @import("../shellfox_cell_layout2.zig");
const profile = @import("../shellfox_terminal_profile.zig");
const wire = @import("../snapshot/io.zig");

pub const Options = extern struct {
    size: usize = @sizeOf(Options),
    abi_version: u32 = 1,
    format_version: u32 = 2,
    wire_profile: u32 = 2,
    layout_abi: u32 = layout.version,
    terminal_profile: u32 = profile.LegacyXterm6,
    flags: u32 = 0,
    runtime_resize_pull_scrollback: u32,
    engine_profile: lib.String,
    max_encoded_bytes: u64,
    max_allocation_bytes: u64,
    max_work_units: u64,
    max_physical_slots: u64,
    max_rows: u64,
    max_text_scalars: u64,
    max_text_bytes: u64,
    max_continuation_bytes: u64,
    max_pages: u32,
    max_records: u32,
    max_visible_columns: u32,
    max_backing_columns: u32,
    max_visible_rows: u32,
    max_pixel_width: u32,
    max_pixel_height: u32,
    reserved: u32 = 0,
};
pub const Report = extern struct {
    size: usize = @sizeOf(Report),
    abi_version: u32 = 1,
    reserved: u32 = 0,
    source_offset: u64 = 0,
    heap_bytes: u64 = 0,
    mapped_bytes: u64 = 0,
    work_units: u64 = 0,
    physical_slots: u64 = 0,
    text_scalars: u64 = 0,
};
pub fn validate(o: Options) !void {
    if (identity.terminal_profile_abi_version != profile.abi_version or identity.snapshot_budget_abi_version != 1)
        return error.UnsupportedProfile;
    if (o.size != @sizeOf(Options)) return error.InvalidOptions;
    if (o.abi_version != 1 or o.format_version != 2 or o.wire_profile != 2 or
        o.layout_abi != layout.version or o.terminal_profile != profile.LegacyXterm6 or
        o.flags != 0 or o.reserved != 0 or o.runtime_resize_pull_scrollback > 1)
        return error.UnsupportedProfile;
    if (o.engine_profile.len != 64 or @intFromPtr(o.engine_profile.ptr) == 0 or
        !identity.matches(o.engine_profile.ptr[0..64])) return error.UnsupportedProfile;
}

pub const Compatibility = struct {
    state: profile.State,
    resize_pull: bool,
};
// Exact fixed-width State fields, never native size_t/padding/pointers.
pub fn encodeCompatibility(w: *std.Io.Writer, o: Options, state: profile.State) !void {
    try validate(o);
    try profile.validateState(&state, o.terminal_profile);
    try w.writeAll(o.engine_profile.ptr[0..64]);
    try wire.writeInt(w, u32, 0x00010001);
    try wire.writeInt(w, u32, state.abi_version);
    try wire.writeInt(w, u32, state.terminal_profile);
    try wire.writeInt(w, u32, state.preceding_join);
    try wire.writeInt(w, u32, o.runtime_resize_pull_scrollback);
    try wire.writeInt(w, u64, 0); // Legacy live image storage explicitly disabled.
    try wire.writeInt(w, u32, 0); // Direct-only image IO, no image decode.
}
pub fn decodeCompatibility(bytes: []const u8, o: Options) !Compatibility {
    try validate(o);
    if (bytes.len != 96 or !std.mem.eql(u8, bytes[0..64], o.engine_profile.ptr[0..64]))
        return error.UnsupportedProfile;
    var r: std.Io.Reader = .fixed(bytes[64..]);
    if (try wire.readInt(&r, u32) != 0x00010001) return error.UnsupportedProfile;
    const state: profile.State = .{
        .abi_version = try wire.readInt(&r, u32),
        .terminal_profile = try wire.readInt(&r, u32),
        .preceding_join = try wire.readInt(&r, u32),
    };
    try profile.validateState(&state, o.terminal_profile);
    const resize = try wire.readInt(&r, u32);
    if (resize != o.runtime_resize_pull_scrollback or resize > 1 or
        try wire.readInt(&r, u64) != 0 or try wire.readInt(&r, u32) != 0)
        return error.UnsupportedProfile;
    return .{ .state = state, .resize_pull = resize == 1 };
}
