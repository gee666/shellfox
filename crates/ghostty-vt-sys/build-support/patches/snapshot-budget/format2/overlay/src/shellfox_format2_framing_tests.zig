//! Standalone framing proof. No canonical layout implementation is substituted.
test {
    _ = @import("terminal/shellfox_snapshot_format2/envelope.zig");
    _ = @import("terminal/shellfox_snapshot_format2/framing_tests.zig");
    _ = @import("terminal/shellfox_snapshot_format2/resources.zig");
}
