package com.shellfox.mobile.rust

import android.text.Editable
import android.text.Selection
import android.text.SpannableStringBuilder
import android.view.KeyEvent
import android.view.View
import android.view.inputmethod.BaseInputConnection

/** The editable holds marked text only, never a second terminal transcript. */
internal class TerminalInputConnection(
  private val view: View, private val input: TerminalInput, private val preedit: (String) -> Unit,
  private val writable: () -> Boolean,
) : BaseInputConnection(view, true) {
  private val marked = SpannableStringBuilder()
  private var monitorCursor = false
  private var anchor = floatArrayOf(0f, 0f, 0f, 0f)
  private var lastAnchor = ""
  override fun getEditable(): Editable = marked

  override fun setComposingText(text: CharSequence?, newCursorPosition: Int): Boolean {
    if (!writable() || (text?.length ?: 0) > 4096) return false
    super.setComposingText(text, newCursorPosition)
    updatePreedit()
    return true
  }

  override fun setComposingRegion(start: Int, end: Int): Boolean {
    if (!boundary(start) || !boundary(end)) return false
    return super.setComposingRegion(start, end)
  }

  override fun commitText(text: CharSequence?, newCursorPosition: Int): Boolean {
    if (!writable()) return false
    val value = text?.toString().orEmpty()
    if (!TerminalInput.validUnicode(value) || value.toByteArray(Charsets.UTF_8).size > 60_000) return false
    clearMarked()
    if (value.isNotEmpty()) input.text("commit", value)
    return true
  }

  override fun finishComposingText(): Boolean {
    if (!writable()) { clearMarked(); return true }
    val value = marked.toString()
    if (!TerminalInput.validUnicode(value)) return false
    clearMarked()
    if (value.isNotEmpty()) input.text("commit", value)
    return true
  }

  override fun deleteSurroundingText(beforeLength: Int, afterLength: Int): Boolean {
    if (!writable() || beforeLength !in 0..256 || afterLength !in 0..256) return false
    if (marked.isNotEmpty()) {
      deleteMarked(beforeLength, afterLength, false)
    } else {
      repeat(beforeLength) { input.key("backspace"); input.key("backspace", "release") }
      repeat(afterLength) { input.key("delete"); input.key("delete", "release") }
    }
    return true
  }

  override fun deleteSurroundingTextInCodePoints(beforeLength: Int, afterLength: Int): Boolean {
    if (marked.isEmpty()) return deleteSurroundingText(beforeLength, afterLength)
    if (!writable() || beforeLength !in 0..256 || afterLength !in 0..256) return false
    deleteMarked(beforeLength, afterLength, true)
    return true
  }

  override fun setSelection(start: Int, end: Int): Boolean {
    if (!boundary(start) || !boundary(end)) return false
    Selection.setSelection(marked, start, end)
    if (monitorCursor) publishCursor()
    return true
  }

  override fun requestCursorUpdates(cursorUpdateMode: Int): Boolean {
    if (!writable() || cursorUpdateMode and (CURSOR_UPDATE_IMMEDIATE or CURSOR_UPDATE_MONITOR).inv() != 0) return false
    monitorCursor = cursorUpdateMode and CURSOR_UPDATE_MONITOR != 0
    if (cursorUpdateMode and CURSOR_UPDATE_IMMEDIATE != 0) publishCursor(force = true)
    return true
  }
  fun updateCursorAnchor(x: Float, top: Float, baseline: Float, bottom: Float) {
    anchor = floatArrayOf(x, top, baseline, bottom)
    if (monitorCursor) publishCursor()
  }
  private fun publishCursor(force: Boolean = false) {
    if (!writable() || !view.isFocused) return
    val location = IntArray(2)
    view.getLocationOnScreen(location)
    val start = Selection.getSelectionStart(marked).coerceAtLeast(0)
    val end = Selection.getSelectionEnd(marked).coerceAtLeast(0)
    val signature = "${anchor.contentToString()}|${location.contentToString()}|$start|$end|$marked"
    if (!force && lastAnchor == signature) return
    lastAnchor = signature
    val matrix = android.graphics.Matrix().apply { setTranslate(location[0].toFloat(), location[1].toFloat()) }
    val builder = android.view.inputmethod.CursorAnchorInfo.Builder().setMatrix(matrix).setSelectionRange(start, end)
      .setInsertionMarkerLocation(anchor[0], anchor[1], anchor[2], anchor[3], android.view.inputmethod.CursorAnchorInfo.FLAG_HAS_VISIBLE_REGION)
    if (marked.isNotEmpty()) builder.setComposingText(0, marked.toString())
    (view.context.getSystemService(android.content.Context.INPUT_METHOD_SERVICE) as? android.view.inputmethod.InputMethodManager)
      ?.updateCursorAnchorInfo(view, builder.build())
  }

  override fun sendKeyEvent(event: KeyEvent): Boolean = writable() && input.hardware(event)
  override fun performEditorAction(editorAction: Int): Boolean {
    if (!writable()) return false
    finishComposingText()
    input.key("enter")
    input.key("enter", "release")
    return true
  }

  override fun closeConnection() { monitorCursor = false; cancelComposition(); super.closeConnection() }
  fun cancelComposition() { clearMarked() }
  private fun boundary(index: Int): Boolean = index in 0..marked.length &&
    (index == 0 || index == marked.length || !marked[index].isLowSurrogate() || !marked[index - 1].isHighSurrogate())
  private fun deleteMarked(before: Int, after: Int, codepoints: Boolean) {
    if (before == 0 && after == 0) return
    val text = marked.toString()
    val selectionStart = Selection.getSelectionStart(marked).coerceIn(0, text.length)
    val selectionEnd = Selection.getSelectionEnd(marked).coerceIn(0, text.length)
    val left = minOf(selectionStart, selectionEnd)
    val right = maxOf(selectionStart, selectionEnd)
    var start = if (codepoints) text.offsetByCodePoints(left, -minOf(before, text.codePointCount(0, left)))
      else (left - before).coerceAtLeast(0)
    var end = if (codepoints) text.offsetByCodePoints(right, minOf(after, text.codePointCount(right, text.length)))
      else (right + after).coerceAtMost(text.length)
    if (start in 1 until text.length && text[start].isLowSurrogate() && text[start - 1].isHighSurrogate()) start--
    if (end in 1 until text.length && text[end - 1].isHighSurrogate() && text[end].isLowSurrogate()) end++
    // Android deletes around a selection, not the selected preedit itself.
    marked.delete(right, end)
    marked.delete(start, left)
    Selection.setSelection(marked, start, start + right - left)
    updatePreedit()
  }
  private fun updatePreedit() {
    val text = marked.toString()
    input.text("preedit", text)
    preedit(text)
    if (monitorCursor) publishCursor()
  }
  private fun clearMarked() {
    removeComposingSpans(marked)
    marked.clear()
    Selection.setSelection(marked, 0)
    input.text("preedit", "")
    preedit("")
    if (monitorCursor) publishCursor()
  }
}
