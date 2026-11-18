package com.shellfox.mobile.rust.observer

import android.app.Activity
import android.graphics.Rect
import android.view.View
import android.view.ViewGroup
import android.widget.EditText
import android.widget.TextView
import com.facebook.react.ReactRootView
import com.facebook.react.uimanager.PointerEvents
import com.facebook.react.uimanager.TouchTargetHelper
import com.facebook.react.views.view.ReactViewGroup
import com.shellfox.mobile.rust.ShellfoxRustTerminalView

/** Main-thread geometry calculation only. Does not create or dispatch events. */
internal object FabricHitProbe {
  fun sample(activity: Activity, ax: AxKeyboard, originalPointBounds: Rect?): HitObservation {
    val tree = descendants(activity.window.decorView)
    val keyboards = tree.filter { it.contentDescription?.toString() == "Keyboard" && it.isShown }
    val terminals = tree.filterIsInstance<ShellfoxRustTerminalView>().filter { it.isShown }
    val keyboard = keyboards.singleOrNull()
    val terminal = terminals.singleOrNull()
    val root = terminal?.let { ancestors(it).filterIsInstance<ReactRootView>().firstOrNull() }
    val rootRect = root?.let(::screenRect) ?: Rect()
    val keyboardRect = keyboard?.let(::screenRect) ?: Rect()
    val terminalRect = terminal?.let(::screenRect) ?: Rect()
    val supported = root != null && ancestors(root).all { it.matrix.isIdentity }
    val point = originalPointBounds?.let { Rect(it.centerX(), it.centerY(), it.centerX() + 1, it.centerY() + 1) } ?: Rect()
    var target = HitTarget.None
    if (root != null && supported && originalPointBounds != null && !originalPointBounds.isEmpty) {
      val location = IntArray(2); root.getLocationOnScreen(location)
      val path = TouchTargetHelper.findTargetPathAndCoordinatesForTouch(
        (point.left - location[0]).toFloat(), (point.top - location[1]).toFloat(), root, FloatArray(2))
      val actual = path.firstNotNullOfOrNull { it.getView() }
      target = when {
        actual == null -> HitTarget.None
        keyboard != null && ancestors(actual).any { it === keyboard } -> HitTarget.KeyboardDescendant
        terminal != null && ancestors(actual).any { it === terminal } -> HitTarget.Terminal
        else -> HitTarget.Other
      }
    }
    val parent = terminal?.parent as? ViewGroup
    val notices = parent?.let { container ->
      (0 until container.childCount).map { container.getChildAt(it) }.filterIsInstance<ReactViewGroup>()
        .filter { it.pointerEvents == PointerEvents.NONE }
    }.orEmpty()
    val notice = notices.singleOrNull()
    // Inspect only the existing controlled notice sibling. Never terminal text,
    // input contents, peer/session titles, accessibility text or arbitrary labels.
    val receipts = notice?.let(::descendants).orEmpty().filterIsInstance<TextView>()
      .filter { it !is EditText }.map { classifyReceipt(it.text) }.filter { it != ReceiptKind.None }
    val receipt = receipts.singleOrNull() ?: ReceiptKind.None
    val visible = Rect(); val hasVisible = notice?.getGlobalVisibleRect(visible) == true
    val noticeOrder = drawOrder(terminal, notice)
    val keyboardOrder = drawOrder(terminal, keyboard)
    return HitObservation(target = target, receipt = receipt, axKeyboardUnique = ax.unique,
      axKeyboardEnabled = ax.enabled, axKeyboardClickable = ax.clickable, axKeyboardFocusable = ax.focusable,
      axKeyboardFocused = ax.focused, axKeyboardSelected = ax.selected,
      nativeKeyboardUnique = keyboards.size == 1, nativeTerminalUnique = terminals.size == 1,
      rootPresent = root != null, rootCoordinatesSupported = supported, rootShown = root?.isShown == true,
      keyboardShown = keyboard?.isShown == true, terminalShown = terminal?.isShown == true,
      keyboardFocused = keyboard?.isFocused == true, terminalFocused = terminal?.isFocused == true,
      keyboardContainsPoint = keyboardRect.contains(point.left, point.top),
      rootContainsPoint = rootRect.contains(point.left, point.top), noticeFound = notice != null,
      noticeShown = notice?.isShown == true, noticeAlphaPositive = notice?.alpha?.let { it > 0f } == true,
      parentClipBoundsPresent = parent?.clipBounds != null, noticeHasVisibleRect = hasVisible,
      noticeSharesTerminalParent = notice != null && notice.parent === parent,
      parentClipChildren = parent?.clipChildren == true, parentClipToPadding = parent?.clipToPadding == true,
      parentOverflowVisible = (parent as? ReactViewGroup)?.overflow == "visible",
      drawingOrderKnown = noticeOrder.first, noticeAfterTerminal = noticeOrder.second,
      keyboardDrawOrderKnown = keyboardOrder.first, keyboardAfterTerminal = keyboardOrder.second,
      rootScreen = rootRect, axKeyboard = ax.bounds ?: Rect(), keyboardScreen = keyboardRect,
      terminalScreen = terminalRect, noticeScreen = notice?.let(::screenRect) ?: Rect(),
      noticeVisible = visible, probeScreen = point, parentClipBounds = parent?.clipBounds ?: Rect())
  }
  internal fun classifyReceipt(value: CharSequence?): ReceiptKind = when (value?.toString()) {
    "Keyboard SHOW. Snapshot since press: notObserved." -> ReceiptKind.ShowNotObserved
    "Keyboard HIDE. Snapshot since press: notObserved." -> ReceiptKind.HideNotObserved
    else -> when {
      value?.startsWith("Keyboard SHOW. Snapshot since press: observed; ") == true -> ReceiptKind.ShowSnapshotObserved
      value?.startsWith("Keyboard HIDE. Snapshot since press: observed; ") == true -> ReceiptKind.HideSnapshotObserved
      else -> ReceiptKind.None
    }
  }
  private fun screenRect(view: View): Rect {
    val location = IntArray(2); view.getLocationOnScreen(location)
    return Rect(location[0], location[1], location[0] + view.width, location[1] + view.height)
  }
  private fun ancestors(view: View): List<View> = buildList {
    var current: View? = view
    while (current != null) { add(current); current = current.parent as? View }
  }
  private fun descendants(view: View): List<View> = buildList {
    val waiting = ArrayDeque<View>(); waiting.add(view)
    while (waiting.isNotEmpty() && size < 512) {
      val current = waiting.removeFirst(); add(current)
      if (current is ShellfoxRustTerminalView || current is EditText) continue
      if (current is ViewGroup) for (index in 0 until current.childCount) waiting.add(current.getChildAt(index))
    }
    if (waiting.isNotEmpty()) clear() // Incomplete tree is unknown, never a false unique match.
  }
  private fun drawOrder(first: View?, second: View?): Pair<Boolean, Boolean> {
    if (first == null || second == null) return false to false
    val firstPath = ancestors(first); val secondPath = ancestors(second)
    val common = firstPath.firstOrNull { it in secondPath } as? ViewGroup ?: return false to false
    val firstChild = firstPath.firstOrNull { it.parent === common } ?: return false to false
    val secondChild = secondPath.firstOrNull { it.parent === common } ?: return false to false
    if (firstChild.z != secondChild.z || android.os.Build.VERSION.SDK_INT < 29) return false to false
    val order = (0 until common.childCount).map { common.getChildAt(common.getChildDrawingOrder(it)) }
    return true to (order.indexOf(secondChild) > order.indexOf(firstChild))
  }
}
