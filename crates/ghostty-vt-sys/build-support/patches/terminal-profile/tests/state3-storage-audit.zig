//! Read-only tests of immutable P4-004 storage. No wire/getter/install code.
const std=@import("std");
const source=@import("state3");
fn successScanWork(primary:*const source.Tabs,alternate:*const source.Tabs) u64 {
    // Reference arithmetic from the separately audited admitBasic source.
    return 2+(@as(u64,primary.maximum_key)+1)+(@as(u64,alternate.maximum_key)+1);
}
test "actual P4-004 tab bits fixed8200 bytes each and valid highest producer key" {
    try std.testing.expectEqual(@as(usize,8200),@sizeOf(@FieldType(source.Tabs,"bits")));
    try std.testing.expectEqual(@as(usize,16400),2*@sizeOf(@FieldType(source.Tabs,"bits")));
    var tabs:source.Tabs=.{};tabs.put(65536);
    try std.testing.expectEqual(@as(u64,1),tabs.bits[1024]);
    try std.testing.expect(tabs.has(65536));
    try std.testing.expectEqual(@as(u32,1),tabs.count);
    tabs.remove(65536);try std.testing.expectEqual(@as(u32,0),tabs.count);
    try std.testing.expectEqual(@as(u32,65536),tabs.maximum_key);
    try std.testing.expect(!tabs.has(65536));
}
test "actual remove retains high water while clear resets it even for same key set" {
    var low:source.Tabs=.{};low.put(0);
    var high:source.Tabs=.{};high.put(0);high.put(500);high.remove(500);
    var alternate:source.Tabs=.{};alternate.put(0);
    try std.testing.expectEqual(low.bits,high.bits);
    try std.testing.expectEqual(low.count,high.count);
    try std.testing.expectEqual(@as(u32,500),high.maximum_key);
    try std.testing.expectEqual(@as(u64,4),successScanWork(&low,&alternate));
    try std.testing.expectEqual(@as(u64,504),successScanWork(&high,&alternate));
    try std.testing.expect(successScanWork(&high,&alternate)>503 and successScanWork(&low,&alternate)<=503);
    high.clear();try std.testing.expectEqual(@as(u32,0),high.maximum_key);
    try std.testing.expectEqual(@as(u32,0),high.count);
}
test "actual setup is interval8 with no owned interval or current URI data" {
    var tabs:source.Tabs=.{};tabs.setup(20,null);
    try std.testing.expectEqual(@as(u32,3),tabs.count);
    try std.testing.expect(tabs.has(0) and tabs.has(8) and tabs.has(16));
    try std.testing.expect(!@hasField(source.State,"tab_stop_interval"));
    try std.testing.expect(@FieldType(source.Pen,"url_known")==bool);
    try std.testing.expect(@FieldType(source.Pen,"url_active")==bool);
    try std.testing.expect(!@hasField(source.Pen,"uri") and !@hasField(source.Pen,"url") and !@hasField(source.Pen,"url_id"));
    var max:source.Tabs=.{};max.put(65536);
    try std.testing.expectEqual(@as(u64,131076),successScanWork(&max,&max));
}
