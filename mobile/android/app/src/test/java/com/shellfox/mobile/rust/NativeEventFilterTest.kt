package com.shellfox.mobile.rust

import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(manifest = Config.NONE, sdk = [24], application = android.app.Application::class)
class NativeEventFilterTest {
  private fun event(id: String, internal: Boolean = true, status: Int = 0, changes: Boolean = false) = JSONObject()
    .put("operationId", id).put("internal", internal).put("status", status)
    .put("connection", JSONObject().put("status", "connected").put("connectionId", "9007199254740993"))
    .put("result", JSONObject().put("ok", true).put("value", JSONObject()
      .put("events", org.json.JSONArray().apply { if (changes) put(JSONObject().put("type", "changed")) })))

  @Test fun suppressesInternalAckWriteResizeSuccessStorms() {
    val filter = NativeEventFilter()
    assertNotNull(filter.filter(event("1", internal = false)))
    repeat(1000) { assertNull(filter.filter(event((it + 2).toString()))) }
  }
  @Test fun explicitPollCompletesItsOperationEvenThoughRustTagsNativePollInternal() {
    val filter = NativeEventFilter()
    filter.expect("9007199254740994")
    val delivered = JSONObject(requireNotNull(filter.filter(event("9007199254740994"))))
    assertFalse(delivered.getBoolean("internal"))
    assertEquals("9007199254740993", delivered.getJSONObject("connection").getString("connectionId"))
  }
  @Test fun liveResetGridDistinguishesHostGeometryFenceOnlyAndStaleWithoutGuessing() {
    assertEquals(ResetGrid.Host(240, 100), ResetGrid.decode(intArrayOf(0, 240, 100)))
    assertEquals(ResetGrid.FenceOnly, ResetGrid.decode(intArrayOf(7, 0, 0)))
    assertEquals(ResetGrid.Stale, ResetGrid.decode(intArrayOf(3, 0, 0)))
    for (invalid in listOf(intArrayOf(0, 512, 512), intArrayOf(3, 240, 100), intArrayOf()))
      assertThrows(IllegalArgumentException::class.java) { ResetGrid.decode(invalid) }
  }
  @Test fun preservesChangedControlEventsAndCoalescesRepeatedInternalErrors() {
    val filter = NativeEventFilter()
    filter.filter(event("1", internal = false))
    assertNotNull(filter.filter(event("2", changes = true)))
    assertNotNull(filter.filter(event("3", status = 10)))
    assertNull(filter.filter(event("4", status = 10)))
    assertNotNull(filter.filter(event("5", internal = false, status = 10)))
  }
  @Test fun everyReattachRequestReachesJsEvenWithARepeatedWriteError() {
    val filter = NativeEventFilter()
    filter.filter(event("1", internal = false))
    fun lost(id: String) = event(id).put("result", JSONObject().put("ok", false)
      .put("error", JSONObject().put("code", "TERMINAL_CONTROL_REQUIRED")))
      .put("reattach", org.json.JSONArray().put(JSONObject().put("tabId", "t")))
    assertNotNull(filter.filter(lost("2")))
    assertNotNull(filter.filter(lost("3")))
    assertNull(filter.filter(lost("4").apply { remove("reattach") }))
  }
}
