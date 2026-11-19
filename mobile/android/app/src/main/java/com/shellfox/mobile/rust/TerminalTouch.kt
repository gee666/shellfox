package com.shellfox.mobile.rust

import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import android.content.Intent
import android.net.Uri
import androidx.core.net.toUri
import android.view.ActionMode
import android.view.GestureDetector
import android.view.Menu
import android.view.MenuItem
import android.view.MotionEvent
import java.util.Locale

internal class TerminalTouch(private val view: ShellfoxRustTerminalView) : GestureDetector.SimpleOnGestureListener() {
  private var mode: ActionMode? = null
  private var anchor: Pair<Int, Int>? = null
  private var link: Uri? = null
  private var accumulatedScroll = 0f
  private var selecting = false
  private var clicked = false
  fun setSelecting(value: Boolean) {
    selecting = value
    if (!value) { finish(); view.select(0, Pair(0, 0)) }
  }
  private val detector = GestureDetector(view.context, this)
  private val clipboard get() = view.context.getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager

  fun handle(event: MotionEvent): Boolean {
    clicked = false
    detector.onTouchEvent(event)
    if (selecting && event.action == MotionEvent.ACTION_DOWN) {
      anchor = view.point(event.x, event.y)
      view.parent?.requestDisallowInterceptTouchEvent(true)
      view.select(1, requireNotNull(anchor))
    }
    if (anchor != null && event.action == MotionEvent.ACTION_MOVE) {
      view.select(1, requireNotNull(anchor), view.point(event.x, event.y))
    }
    if (event.action == MotionEvent.ACTION_UP || event.action == MotionEvent.ACTION_CANCEL) {
      accumulatedScroll = 0f
      if (anchor != null && event.action == MotionEvent.ACTION_UP) view.selectionChanged()
      if (selecting) { anchor = null; view.parent?.requestDisallowInterceptTouchEvent(false) }
    }
    return clicked
  }
  override fun onDown(event: MotionEvent): Boolean = true
  override fun onSingleTapUp(event: MotionEvent): Boolean {
    if (selecting) return true
    if (mode != null) finish() else {
      view.fileLinkAt(event.x, event.y)
      view.pointer(event.x, event.y)
      clicked = true
    }
    return true
  }
  override fun onScroll(first: MotionEvent?, current: MotionEvent, distanceX: Float, distanceY: Float): Boolean {
    if (anchor != null) return true
    accumulatedScroll += distanceY / view.cellHeight
    val rows = accumulatedScroll.toInt()
    if (rows != 0) {
      accumulatedScroll -= rows
      view.scroll(rows, current.x, current.y)
      view.parent?.requestDisallowInterceptTouchEvent(true)
    }
    return true
  }
  override fun onLongPress(event: MotionEvent) {
    finish()
    val point = view.point(event.x, event.y)
    anchor = point
    view.select(2, point)
    view.selectionChanged()
    mode = view.startActionMode(object : ActionMode.Callback {
      override fun onCreateActionMode(action: ActionMode, menu: Menu): Boolean {
        menu.add(0, android.R.id.copy, 0, "Copy")
        menu.add(0, android.R.id.selectAll, 1, "Select all")
        if (view.acceptsInput) menu.add(0, android.R.id.paste, 2, "Paste")
        view.hyperlink(point) { value ->
          link = safeLink(value)
          if (mode === action && link != null) { menu.add(0, OPEN_LINK, 3, "Open link"); action.invalidate() }
        }
        return true
      }
      override fun onPrepareActionMode(action: ActionMode, menu: Menu) = false
      override fun onActionItemClicked(action: ActionMode, item: MenuItem): Boolean {
        when (item.itemId) {
          android.R.id.copy -> { copy(); action.finish() }
          android.R.id.selectAll -> { view.select(4, Pair(0, 0)); view.selectionChanged() }
          android.R.id.paste -> {
            clipboard.primaryClip?.getItemAt(0)?.text?.toString()?.let(view::paste)
            action.finish()
          }
          OPEN_LINK -> {
            link?.let { uri ->
              try { view.context.startActivity(Intent(Intent.ACTION_VIEW, uri).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)) }
              catch (_: android.content.ActivityNotFoundException) { view.reportError(2) }
              catch (_: SecurityException) { view.reportError(2) }
            }
            action.finish()
          }
          else -> return false
        }
        return true
      }
      override fun onDestroyActionMode(action: ActionMode) {
        mode = null; anchor = null; link = null
        view.select(0, Pair(0, 0))
        view.selectionChanged()
        view.parent?.requestDisallowInterceptTouchEvent(false)
      }
    }, ActionMode.TYPE_FLOATING)
    if (mode == null) anchor = null
  }
  fun copy() { view.copy { text ->
    // Android clipboard IPC has a shared Binder transaction limit. Refuse an
    // oversized selection explicitly rather than silently truncate or crash.
    if (text.length > 262144 || text.toByteArray().size > 524288) view.reportError(2)
    else try { clipboard.setPrimaryClip(ClipData.newPlainText("Terminal", text)) }
    catch (_: RuntimeException) { view.reportError(2) }
  } }
  fun finish() { mode?.finish(); anchor = null; link = null }

  companion object {
    private const val OPEN_LINK = 4
    fun safeLink(value: String): Uri? {
      if (value.length > 8192 || value.any { it.isISOControl() }) return null
      val uri = value.toUri()
      return if (uri.scheme?.lowercase(Locale.ROOT) in setOf("https", "http") &&
        !uri.host.isNullOrEmpty() && uri.userInfo == null) uri else null
    }
  }
}
