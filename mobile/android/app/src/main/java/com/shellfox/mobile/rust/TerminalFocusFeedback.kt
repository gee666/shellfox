package com.shellfox.mobile.rust

import com.facebook.react.bridge.Arguments
import com.facebook.react.bridge.WritableMap

internal enum class FocusActionSource(val wire: String) { KEYBOARD("keyboard"), SURFACE("surface"), ATTACHMENT("attachment") }
internal enum class FocusRequestResult(val wire: String) { NOT_ATTEMPTED("notAttempted"), ACCEPTED("accepted"), REJECTED("rejected") }
/** Internal identity only. No sink, epoch or tag identifier crosses the UI event. */
internal class FocusActionFence(private val handle: String, private val sink: Any?, private val epoch: Long?) {
  fun current(handle: String, sink: Any?, epoch: Long?) = this.handle == handle && this.sink === sink && this.epoch == epoch
}
internal data class TerminalFocusFeedback(
  val source: FocusActionSource,
  val attached: Boolean,
  val windowFocused: Boolean,
  val uiThread: Boolean,
  val inputAllowed: Boolean,
  val frameAvailable: Boolean,
  val focusRequest: FocusRequestResult,
  val focused: Boolean,
  val keyboardRequest: FocusRequestResult,
) {
  fun map(): WritableMap = Arguments.createMap().apply {
    putString("platform", "android"); putString("source", source.wire)
    putBoolean("attached", attached); putBoolean("windowFocused", windowFocused); putBoolean("uiThread", uiThread)
    putBoolean("inputAllowed", inputAllowed); putBoolean("frameAvailable", frameAvailable)
    putString("focusRequest", focusRequest.wire); putBoolean("focused", focused)
    putString("keyboardRequest", keyboardRequest.wire)
  }
}
