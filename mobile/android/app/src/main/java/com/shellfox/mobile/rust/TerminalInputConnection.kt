package com.shellfox.mobile.rust

import android.text.Editable
import android.text.Selection
import android.text.SpannableStringBuilder
import android.view.KeyEvent
import android.view.View
import android.view.inputmethod.BaseInputConnection
import android.view.inputmethod.InputMethodManager

/**
 * The keyboard's own small editor. It holds the text typed since the last
 * Enter or key, so the keyboard can autocorrect, swipe and suggest on it.
 * After every keyboard call, its committed text goes to Rust, which sends the
 * terminal the edit (backspaces + new text). Composing text stays a preedit.
 */
internal class TerminalInputConnection(
  private val view: View, private val input: TerminalInput, private val preedit: (String) -> Unit,
  private val writable: () -> Boolean,
) : BaseInputConnection(view, true) {
  private val editor = SpannableStringBuilder()
  private var reported = ""
  private var monitorCursor = false
  private var anchor = floatArrayOf(0f, 0f, 0f, 0f)
  private var lastAnchor = ""
  private var recomposed = false
  private var batch = 0
  private val ime get() = view.context.getSystemService(android.content.Context.INPUT_METHOD_SERVICE) as? InputMethodManager
  override fun getEditable(): Editable = editor

  private inline fun edit(action: () -> Boolean): Boolean {
    if (!writable()) return false
    val handled = action()
    report()
    if (batch == 0) publishSelection()
    return handled
  }
  // The keyboard learns the result of its edits only from updateSelection, as
  // from a text field. Without it, Gboard can keep a tapped suggestion as
  // composing text until the next word.
  override fun beginBatchEdit(): Boolean { batch++; return true }
  override fun endBatchEdit(): Boolean {
    if (batch > 0 && --batch == 0) publishSelection()
    return batch > 0
  }
  private fun publishSelection() {
    val composingStart = getComposingSpanStart(editor)
    val composingEnd = getComposingSpanEnd(editor)
    ime?.updateSelection(view, Selection.getSelectionStart(editor).coerceAtLeast(0),
      Selection.getSelectionEnd(editor).coerceAtLeast(0), composingStart, composingEnd)
  }
  override fun setComposingText(text: CharSequence?, newCursorPosition: Int) = edit {
    super.setComposingText(text, newCursorPosition)
  }
  // The keyboard reopens committed text (a suggestion, a correction).
  override fun setComposingRegion(start: Int, end: Int) = edit {
    super.setComposingRegion(start, end).also { recomposed = start != end }
  }
  override fun commitText(text: CharSequence?, newCursorPosition: Int): Boolean {
    val value = text?.toString().orEmpty()
    if (!writable() || !TerminalInput.validUnicode(value)) return false
    // A keyboard's clipboard paste of a long text is a paste, not typing.
    if (value.length > LONG_COMMIT) { finishAndClear(); input.paste(value); return true }
    return edit { super.commitText(text, newCursorPosition) }
  }
  override fun finishComposingText(): Boolean {
    if (!writable()) { clear(); return true }
    return edit { super.finishComposingText() }
  }
  override fun deleteSurroundingText(beforeLength: Int, afterLength: Int): Boolean {
    if (!writable() || beforeLength < 0 || afterLength < 0) return false
    // Nothing left in the editor: the deletion reaches older terminal text.
    if (editor.isEmpty()) {
      repeat(minOf(beforeLength, 256)) { input.tap(TerminalInput.BACKSPACE, 0) }
      return true
    }
    return edit { super.deleteSurroundingText(beforeLength, afterLength) }
  }
  override fun deleteSurroundingTextInCodePoints(beforeLength: Int, afterLength: Int): Boolean {
    if (!writable() || beforeLength < 0 || afterLength < 0) return false
    if (editor.isEmpty()) return deleteSurroundingText(beforeLength, afterLength)
    return edit { super.deleteSurroundingTextInCodePoints(beforeLength, afterLength) }
  }
  override fun setSelection(start: Int, end: Int) = edit { super.setSelection(start, end) }

  override fun sendKeyEvent(event: KeyEvent): Boolean {
    if (!writable()) return false
    // Soft-keyboard Backspace edits the editor while it has text, then the terminal.
    if (event.keyCode == KeyEvent.KEYCODE_DEL && event.action == KeyEvent.ACTION_DOWN && editor.isNotEmpty()) {
      val end = Selection.getSelectionEnd(editor).coerceIn(0, editor.length)
      val start = Selection.getSelectionStart(editor).coerceIn(0, editor.length)
      return edit {
        if (start != end) editor.delete(minOf(start, end), maxOf(start, end))
        else if (end > 0) editor.delete(Character.offsetByCodePoints(editor, end, -1), end)
        true
      }
    }
    if (event.action == KeyEvent.ACTION_DOWN) finishAndClear()
    return input.hardware(event, preedit)
  }
  override fun performEditorAction(editorAction: Int): Boolean {
    if (!writable()) return false
    finishAndClear()
    input.tap(TerminalInput.ENTER, 0)
    return true
  }

  /** What is typed so far stays; the next word starts in an empty editor. */
  fun finishAndClear() {
    if (editor.isEmpty() && reported.isEmpty()) return
    super.finishComposingText()
    report()
    clear()
  }
  /** Drops a fresh preedit, but keeps a reopened word: the terminal already lost it. */
  fun cancelComposition() = if (recomposed) finishAndClear() else clear()
  override fun closeConnection() { monitorCursor = false; batch = 0; clear(); super.closeConnection() }

  private fun report() {
    val start = getComposingSpanStart(editor)
    val end = getComposingSpanEnd(editor)
    val composing = start in 0..end && end <= editor.length && start != end
    val committed = if (composing) editor.substring(0, start) + editor.substring(end) else editor.toString()
    val marked = if (composing) editor.substring(start, end) else ""
    val sticky = input.stickyModifiers != 0
    val typed = (committed + marked).removePrefix(reported)
    if (sticky && (committed + marked).startsWith(reported) && typed.isNotEmpty()) {
      // A held key-bar modifier applies to the characters just typed.
      typed.codePoints().toArray().forEach(input::chord)
      clear()
      return
    }
    if (!composing) recomposed = false
    if (committed != reported) input.editor(committed)
    reported = committed
    preedit(marked)
    // Enter and Tab change the line; start the next text in an empty editor.
    if (committed.contains('\n') || committed.contains('\t')) clear()
    else if (editor.length > MAX_EDITOR) {
      if (marked.isNotEmpty()) input.editor(editor.toString())
      clear()
    }
    if (monitorCursor) publishCursor()
  }
  private fun clear() {
    removeComposingSpans(editor)
    editor.clear()
    Selection.setSelection(editor, 0)
    reported = ""
    recomposed = false
    preedit("")
    input.editorReset()
    // The keyboard reads its context again from the now empty editor.
    ime?.updateSelection(view, 0, 0, -1, -1)
    if (monitorCursor) publishCursor()
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
    val start = Selection.getSelectionStart(editor).coerceAtLeast(0)
    val end = Selection.getSelectionEnd(editor).coerceAtLeast(0)
    val signature = "${anchor.contentToString()}|${location.contentToString()}|$start|$end|$editor"
    if (!force && lastAnchor == signature) return
    lastAnchor = signature
    val matrix = android.graphics.Matrix().apply { setTranslate(location[0].toFloat(), location[1].toFloat()) }
    val builder = android.view.inputmethod.CursorAnchorInfo.Builder().setMatrix(matrix).setSelectionRange(start, end)
      .setInsertionMarkerLocation(anchor[0], anchor[1], anchor[2], anchor[3], android.view.inputmethod.CursorAnchorInfo.FLAG_HAS_VISIBLE_REGION)
    val composingStart = getComposingSpanStart(editor)
    if (composingStart >= 0) builder.setComposingText(composingStart, editor.subSequence(composingStart, getComposingSpanEnd(editor)))
    ime?.updateCursorAnchorInfo(view, builder.build())
  }
  private companion object {
    const val LONG_COMMIT = 1024
    const val MAX_EDITOR = 4096
  }
}
