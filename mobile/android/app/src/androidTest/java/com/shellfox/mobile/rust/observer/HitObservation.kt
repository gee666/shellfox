package com.shellfox.mobile.rust.observer

import android.graphics.Rect
import org.json.JSONObject

internal enum class HitTarget { KeyboardDescendant, Terminal, Other, None }
internal enum class ReceiptKind { None, ShowNotObserved, HideNotObserved, ShowSnapshotObserved, HideSnapshotObserved }
internal data class AxKeyboard(val bounds: Rect? = null, val unique: Boolean = false,
  val enabled: Boolean = false, val clickable: Boolean = false, val focusable: Boolean = false,
  val focused: Boolean = false, val selected: Boolean = false)

/** Output has fixed enums, booleans and rectangles only. Never native tags/text. */
internal data class HitObservation(
  val target: HitTarget = HitTarget.None,
  val receipt: ReceiptKind = ReceiptKind.None,
  val axKeyboardUnique: Boolean = false,
  val axKeyboardEnabled: Boolean = false,
  val axKeyboardClickable: Boolean = false,
  val axKeyboardFocusable: Boolean = false,
  val axKeyboardFocused: Boolean = false,
  val axKeyboardSelected: Boolean = false,
  val nativeKeyboardUnique: Boolean = false,
  val nativeTerminalUnique: Boolean = false,
  val rootPresent: Boolean = false,
  val rootCoordinatesSupported: Boolean = false,
  val rootShown: Boolean = false,
  val keyboardShown: Boolean = false,
  val terminalShown: Boolean = false,
  val keyboardFocused: Boolean = false,
  val terminalFocused: Boolean = false,
  val keyboardContainsPoint: Boolean = false,
  val rootContainsPoint: Boolean = false,
  val noticeFound: Boolean = false,
  val noticeShown: Boolean = false,
  val noticeAlphaPositive: Boolean = false,
  val parentClipBoundsPresent: Boolean = false,
  val noticeHasVisibleRect: Boolean = false,
  val noticeSharesTerminalParent: Boolean = false,
  val parentClipChildren: Boolean = false,
  val parentClipToPadding: Boolean = false,
  val parentOverflowVisible: Boolean = false,
  val drawingOrderKnown: Boolean = false,
  val noticeAfterTerminal: Boolean = false,
  val keyboardDrawOrderKnown: Boolean = false,
  val keyboardAfterTerminal: Boolean = false,
  val rootScreen: Rect = Rect(),
  val axKeyboard: Rect = Rect(),
  val keyboardScreen: Rect = Rect(),
  val terminalScreen: Rect = Rect(),
  val noticeScreen: Rect = Rect(),
  val noticeVisible: Rect = Rect(),
  val probeScreen: Rect = Rect(),
  val parentClipBounds: Rect = Rect(),
) {
  fun json(): JSONObject {
    val out = JSONObject().put("target", target.name).put("receipt", receipt.name)
    val flags = mapOf("axKeyboardUnique" to axKeyboardUnique, "axKeyboardEnabled" to axKeyboardEnabled,
      "axKeyboardClickable" to axKeyboardClickable, "axKeyboardFocusable" to axKeyboardFocusable,
      "axKeyboardFocused" to axKeyboardFocused, "axKeyboardSelected" to axKeyboardSelected, "nativeKeyboardUnique" to nativeKeyboardUnique,
      "nativeTerminalUnique" to nativeTerminalUnique, "rootPresent" to rootPresent,
      "rootCoordinatesSupported" to rootCoordinatesSupported, "rootShown" to rootShown,
      "keyboardShown" to keyboardShown, "terminalShown" to terminalShown,
      "keyboardFocused" to keyboardFocused, "terminalFocused" to terminalFocused,
      "keyboardContainsPoint" to keyboardContainsPoint, "rootContainsPoint" to rootContainsPoint,
      "noticeFound" to noticeFound, "noticeShown" to noticeShown, "noticeAlphaPositive" to noticeAlphaPositive,
      "parentClipBoundsPresent" to parentClipBoundsPresent,
      "noticeHasVisibleRect" to noticeHasVisibleRect, "noticeSharesTerminalParent" to noticeSharesTerminalParent,
      "parentClipChildren" to parentClipChildren, "parentClipToPadding" to parentClipToPadding,
      "parentOverflowVisible" to parentOverflowVisible, "drawingOrderKnown" to drawingOrderKnown,
      "noticeAfterTerminal" to noticeAfterTerminal, "keyboardDrawOrderKnown" to keyboardDrawOrderKnown,
      "keyboardAfterTerminal" to keyboardAfterTerminal)
    flags.forEach { (key, value) -> out.put(key, value) }
    mapOf("rootScreen" to rootScreen, "axKeyboard" to axKeyboard, "keyboardScreen" to keyboardScreen,
      "terminalScreen" to terminalScreen, "noticeScreen" to noticeScreen, "noticeVisible" to noticeVisible,
      "probeScreen" to probeScreen, "parentClipBounds" to parentClipBounds).forEach { (key, rect) ->
      out.put(key, JSONObject().put("left", rect.left).put("top", rect.top).put("right", rect.right).put("bottom", rect.bottom))
    }
    return out
  }
}
