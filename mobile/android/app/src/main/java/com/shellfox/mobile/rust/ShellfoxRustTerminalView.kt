package com.shellfox.mobile.rust

import android.content.Context
import android.graphics.Canvas
import android.os.Bundle
import android.text.InputType
import android.view.KeyEvent
import android.view.MotionEvent
import android.view.View
import android.view.accessibility.AccessibilityEvent
import android.view.accessibility.AccessibilityNodeInfo
import android.view.inputmethod.EditorInfo
import android.view.inputmethod.InputConnection
import android.view.inputmethod.InputMethodManager
import androidx.core.graphics.withClip
import com.facebook.react.bridge.Arguments
import com.facebook.react.bridge.ReactContext
import com.facebook.react.bridge.WritableMap
import com.facebook.react.uimanager.UIManagerHelper
import com.facebook.react.uimanager.events.Event
import org.json.JSONObject

private const val MAX_PASTE = 1 shl 20 // UTF-16 units, sent in write-sized pieces
internal class ShellfoxRustTerminalView(context: Context) : View(context) {
  private val renderer = TerminalRenderer(resources.displayMetrics.density)
  private var terminalHandle = ""
  private var sink: Sink? = null
  var readOnly = false
    set(value) {
      if (value && !field) { connection?.cancelComposition(); input.reset() }
      field = value
      resizeGrid()
      refreshInputContext()
    }
  private val inputRefresh = InputRefreshGate()
  val acceptsInput: Boolean get() = !readOnly
  private var focusRequested = false
  private var preedit = ""
  private var connection: TerminalInputConnection? = null
  private val input = TerminalInput { json ->
    val active = sink
    if (acceptsInput && active != null) {
      TerminalCoordinator.input(terminalHandle, active, active.inputEpoch, json)
    }
  }
  private val touch = TerminalTouch(this)

  private inner class Sink(val handle: String) : TerminalCoordinator.TerminalSink {
    val frames = TerminalFrames()
    @Volatile var inputEpoch = 0L
    @Volatile var inputAvailable = false
    @Volatile var inputReset: Any = Any()
    override fun resetInput(epoch: Long) {
      inputReset = Any()
      val originalReset = inputReset
      inputEpoch = epoch
      inputAvailable = false
      frames.clear()
      postInvalidateOnAnimation()
      post { if (sink === this) {
        connection?.cancelComposition()
        touch.finish()
        input.reset(releaseHeld = false)
        if (inputReset === originalReset) { inputRefresh.invalidate(); refreshInputContext() }
      } }
    }
    override fun typedInput(json: JSONObject) { post { if (sink === this) { connection?.finishAndClear(); input.forward(json) } } }
    override fun userInput() { post { if (sink === this) emit("topInput", Arguments.createMap().apply {
      putString("terminalHandle", handle)
    }) } }
    override fun appearance(appearance: TerminalAppearance) { post {
      if (sink === this) { renderer.appearance = appearance; invalidate() }
    } }
    override fun frame(frame: TerminalFrame) {
      val originalEpoch = inputEpoch
      val originalReset = inputReset
      frames.publish(frame)
      if (!inputAvailable) {
        inputAvailable = true
        post { if (sink === this && inputEpoch == originalEpoch && inputReset === originalReset && inputAvailable) refreshInputContext() }
      }
      postInvalidateOnAnimation()
    }
    override fun state(state: String) { post { if (sink === this) { emit("topState", Arguments.createMap().apply {
      putString("terminalHandle", handle); putString("state", state)
    }); if (state == "ready") resizeGrid() } } }
    override fun error(code: Int) { post { if (sink === this) emit("topError", Arguments.createMap().apply {
      putString("terminalHandle", handle); putInt("code", code)
    }) } }
  }

  init {
    renderer.fontSize(14f, resources.displayMetrics)
    isFocusable = true
    isFocusableInTouchMode = true
    importantForAccessibility = IMPORTANT_FOR_ACCESSIBILITY_YES
    contentDescription = "Terminal"
    setWillNotDraw(false)
  }

  fun setTerminalHandle(value: String?) {
    val next = value.orEmpty()
    if (next == terminalHandle) return
    unbind()
    terminalHandle = next
    if (isAttachedToWindow) bind()
    invalidate()
  }
  fun setFocused(value: Boolean) {
    focusRequested = value
    if (!isAttachedToWindow) {
      if (value) focusFeedback(FocusActionSource.KEYBOARD, FocusRequestResult.NOT_ATTEMPTED, FocusRequestResult.NOT_ATTEMPTED)
      return
    }
    if (value) focusAction(FocusActionSource.KEYBOARD) else {
      clearFocus()
      (context.getSystemService(Context.INPUT_METHOD_SERVICE) as? InputMethodManager)?.hideSoftInputFromWindow(windowToken, 0)
    }
  }
  fun setSelecting(value: Boolean) { touch.setSelecting(value) }
  fun selectionChanged() {
    copy { text ->
      val bytes = text.toByteArray()
      val selected = Charsets.UTF_8.newDecoder().onMalformedInput(java.nio.charset.CodingErrorAction.IGNORE)
        .decode(java.nio.ByteBuffer.wrap(bytes, 0, minOf(bytes.size, 65536))).toString()
      emit("topSelection", Arguments.createMap().apply {
        putString("terminalHandle", terminalHandle); putString("text", selected)
      })
    }
  }
  fun setModifiers(value: Int) { input.stickyModifiers = if (value in 0..15) value else 0 }
  fun setFontSize(value: Float) {
    renderer.fontSize(if (value.isFinite() && value in 6f..48f) value else 14f, resources.displayMetrics)
    resizeGrid()
    invalidate()
  }
  override fun onAttachedToWindow() { super.onAttachedToWindow(); bind(); if (focusRequested) focusAction(FocusActionSource.ATTACHMENT) }
  override fun onDetachedFromWindow() { unbind(); super.onDetachedFromWindow() }
  fun dispose() { unbind(); terminalHandle = "" }
  private fun bind() {
    if (sink != null || terminalHandle.isEmpty()) return
    val active = Sink(terminalHandle)
    sink = active
    TerminalCoordinator.attach(terminalHandle, active)
  }
  private fun unbind() {
    inputRefresh.invalidate()
    connection?.cancelComposition()
    input.reset()
    touch.finish()
    select(0, Pair(0, 0))
    sink?.let { TerminalCoordinator.detach(it.handle, it) }
    sink = null
    preedit = ""
  }

  private fun inputRefreshState(): InputRefreshState? {
    val active = sink ?: return null
    return InputRefreshState(terminalHandle, active, active.inputEpoch, active.inputReset,
      active.inputAvailable && active.frames.acquire() != null, isAttachedToWindow, hasWindowFocus(), isFocused, acceptsInput)
  }
  private fun refreshInputContext() {
    inputRefresh.update(inputRefreshState(), { post(it) }, ::inputRefreshState) {
      val ime = context.getSystemService(Context.INPUT_METHOD_SERVICE) as? InputMethodManager
      checkNotNull(ime).restartInput(this)
    }
  }
  override fun onWindowFocusChanged(hasWindowFocus: Boolean) { super.onWindowFocusChanged(hasWindowFocus); refreshInputContext() }

  override fun onDraw(canvas: Canvas) {
    super.onDraw(canvas)
    sink?.frames?.acquire()?.let { frame ->
      canvas.withClip(0, 0, width, height) { renderer.draw(this, frame, null, preedit) }
      val y = frame.cursorY * renderer.cellHeight
      connection?.updateCursorAnchor(frame.cursorX * renderer.cellWidth, y, y + renderer.baseline, y + renderer.cellHeight)
    }
  }
  override fun onSizeChanged(w: Int, h: Int, oldw: Int, oldh: Int) { resizeGrid() }
  private fun resizeGrid() {
    val active = sink ?: return
    if (width < 1 || height < 1) return
    val cols = (width / renderer.cellWidth).toInt().coerceIn(2, 500)
    val rows = (height / renderer.cellHeight).toInt().coerceIn(2, minOf(500, 65536 / cols))
    TerminalCoordinator.resize(terminalHandle, active, cols, rows, acceptsInput)
  }
  override fun onCheckIsTextEditor(): Boolean = acceptsInput
  override fun onCreateInputConnection(outAttrs: EditorInfo): InputConnection? {
    if (!acceptsInput) return null
    connection?.cancelComposition()
    // As 0.9.15's WebView textarea: suggestions, swipe and voice on; no
    // autocorrect or auto-capitalization. Replacements are edits, not retyping.
    outAttrs.inputType = InputType.TYPE_CLASS_TEXT or InputType.TYPE_TEXT_FLAG_MULTI_LINE
    outAttrs.imeOptions = EditorInfo.IME_FLAG_NO_EXTRACT_UI or EditorInfo.IME_FLAG_NO_FULLSCREEN
    outAttrs.initialSelStart = 0
    outAttrs.initialSelEnd = 0
    val active = sink
    val epoch = active?.inputEpoch
    return TerminalInputConnection(this, input, ::showPreedit, {
      acceptsInput && isFocused && active != null && sink === active && active.inputAvailable && active.inputEpoch == epoch
    }).also { connection = it }
  }
  override fun onKeyDown(keyCode: Int, event: KeyEvent): Boolean {
    if (!acceptsInput) return super.onKeyDown(keyCode, event)
    // A key for the terminal ends the keyboard's current word; system keys do not.
    if (TerminalInput.physical(keyCode) != null || event.unicodeChar != 0) connection?.finishAndClear()
    return input.hardware(event, ::showPreedit) || super.onKeyDown(keyCode, event)
  }
  override fun onKeyUp(keyCode: Int, event: KeyEvent): Boolean =
    (acceptsInput && input.hardware(event, ::showPreedit)) || super.onKeyUp(keyCode, event)
  private fun showPreedit(text: String) { preedit = text; invalidate() }
  override fun onFocusChanged(gainFocus: Boolean, direction: Int, previous: android.graphics.Rect?) {
    super.onFocusChanged(gainFocus, direction, previous)
    if (!gainFocus) { connection?.cancelComposition(); input.reset() }
    refreshInputContext()
  }
  override fun onTouchEvent(event: MotionEvent): Boolean {
    if (touch.handle(event)) performClick()
    return true
  }
  override fun onGenericMotionEvent(event: MotionEvent): Boolean {
    if (event.action == MotionEvent.ACTION_SCROLL) {
      scroll((-event.getAxisValue(MotionEvent.AXIS_VSCROLL) * 3).toInt(), event.x, event.y)
      return true
    }
    return super.onGenericMotionEvent(event)
  }
  override fun performClick(): Boolean = focusAction(FocusActionSource.SURFACE)
  private fun focusAction(source: FocusActionSource): Boolean {
    val original = sink
    val fence = FocusActionFence(terminalHandle, original, original?.inputEpoch)
    super.performClick()
    val focused = requestFocus() // Existing ordinary request, exactly once.
    val ime = if (acceptsInput) (context.getSystemService(Context.INPUT_METHOD_SERVICE) as? InputMethodManager)
      ?.showSoftInput(this, InputMethodManager.SHOW_IMPLICIT) else null
    focusFeedback(source, if (focused) FocusRequestResult.ACCEPTED else FocusRequestResult.REJECTED,
      when (ime) { true -> FocusRequestResult.ACCEPTED; false -> FocusRequestResult.REJECTED; null -> FocusRequestResult.NOT_ATTEMPTED }, fence)
    return true
  }
  private fun focusFeedback(source: FocusActionSource, request: FocusRequestResult, keyboard: FocusRequestResult,
    fence: FocusActionFence = FocusActionFence(terminalHandle, sink, sink?.inputEpoch)) {
    val handle = terminalHandle
    val result = TerminalFocusFeedback(source, isAttachedToWindow, hasWindowFocus(), android.os.Looper.myLooper() == android.os.Looper.getMainLooper(), acceptsInput,
      sink?.inputAvailable == true, request, isFocused, keyboard)
    post {
      if (handle.isNotEmpty() && fence.current(terminalHandle, sink, sink?.inputEpoch))
        emit("topState", Arguments.createMap().apply {
          putString("terminalHandle", handle); putString("state", "focusResult"); putMap("focusResult", result.map())
        })
    }
  }

  fun scroll(rows: Int, x: Float, y: Float) {
    if (rows == 0) return
    sink?.let { TerminalCoordinator.scroll(terminalHandle, it, rows.coerceIn(-32, 32), mouseX(x), mouseY(y),
      renderer.cellWidth.toInt().coerceAtLeast(1), renderer.cellHeight.toInt().coerceAtLeast(1), acceptsInput) }
  }
  fun pointer(x: Float, y: Float) {
    if (!acceptsInput) return
    sink?.let { TerminalCoordinator.pointer(terminalHandle, it, mouseX(x), mouseY(y),
      renderer.cellWidth.toInt().coerceAtLeast(1), renderer.cellHeight.toInt().coerceAtLeast(1)) }
  }
  private fun mouseX(x: Float) = x / renderer.cellWidth * renderer.cellWidth.toInt().coerceAtLeast(1)
  private fun mouseY(y: Float) = y / renderer.cellHeight * renderer.cellHeight.toInt().coerceAtLeast(1)
  val cellHeight: Float get() = renderer.cellHeight
  fun point(x: Float, y: Float, frame: TerminalFrame? = sink?.frames?.acquire()) =
    Pair((x / renderer.cellWidth).toInt().coerceIn(0, (frame?.cols ?: 1) - 1),
      (y / renderer.cellHeight).toInt().coerceIn(0, (frame?.rows ?: 1) - 1))
  fun fileLinkAt(x: Float, y: Float) {
    val frame = sink?.frames?.acquire() ?: return
    if (x < 0 || y < 0 || x >= frame.cols * renderer.cellWidth || y >= frame.rows * renderer.cellHeight) return
    val point = point(x, y, frame)
    val cell = point.second * frame.cols + point.first
    val start = if (frame.cells[cell].width == 0 && point.first > 0 && frame.cells[cell - 1].width == 2) cell - 1 else cell
    link(Pair(start % frame.cols, point.second), frame.cells[start].flags and 256 == 0, frame) { text ->
      if (text.isNotBlank() && text.length <= 8192) emit("topFileLink", Arguments.createMap().apply {
        putString("terminalHandle", terminalHandle); putString("text", text)
      })
    }
  }
  fun select(kind: Int, start: Pair<Int, Int>, end: Pair<Int, Int> = start) {
    sink?.let { TerminalCoordinator.select(terminalHandle, it, kind, start.first, start.second, end.first, end.second) }
  }
  fun copy(done: (String) -> Unit) {
    val active = sink ?: return
    val reset = active.inputReset
    TerminalCoordinator.copy(terminalHandle, active) { text -> post { if (sink === active && active.inputReset === reset) done(text) } }
  }
  fun hyperlink(point: Pair<Int, Int>, done: (String) -> Unit) = link(point, false, done = done)
  private fun link(point: Pair<Int, Int>, plaintext: Boolean, frame: TerminalFrame? = null, done: (String) -> Unit) {
    val active = sink ?: return
    val reset = active.inputReset
    val stillVisible = frame?.let { tapped -> { active.frames.acquire() === tapped && active.inputReset === reset } }
    TerminalCoordinator.link(terminalHandle, active, point.first, point.second, plaintext, stillVisible) { target ->
      post { if (sink === active && active.inputReset === reset && stillVisible?.invoke() != false) done(target) }
    }
  }
  fun paste(text: String) {
    if (!acceptsInput) return
    if (!TerminalInput.validUnicode(text) || text.length > MAX_PASTE) reportError(2) else {
      connection?.finishAndClear()
      input.paste(text)
    }
  }

  override fun onInitializeAccessibilityNodeInfo(info: AccessibilityNodeInfo) {
    super.onInitializeAccessibilityNodeInfo(info)
    info.className = "android.widget.EditText"
    info.isEditable = acceptsInput
    info.isMultiLine = true
    info.isScrollable = true
    info.text = sink?.frames?.acquire()?.text()?.take(32768).orEmpty()
    info.addAction(AccessibilityNodeInfo.AccessibilityAction.ACTION_SCROLL_FORWARD)
    info.addAction(AccessibilityNodeInfo.AccessibilityAction.ACTION_SCROLL_BACKWARD)
    info.addAction(AccessibilityNodeInfo.AccessibilityAction.ACTION_COPY)
    if (acceptsInput) info.addAction(AccessibilityNodeInfo.AccessibilityAction.ACTION_SET_TEXT)
  }
  override fun performAccessibilityAction(action: Int, args: Bundle?): Boolean {
    when (action) {
      AccessibilityNodeInfo.ACTION_SCROLL_FORWARD -> scroll(5, 0f, 0f)
      AccessibilityNodeInfo.ACTION_SCROLL_BACKWARD -> scroll(-5, 0f, 0f)
      AccessibilityNodeInfo.ACTION_COPY -> touch.copy()
      AccessibilityNodeInfo.ACTION_SET_TEXT -> {
        if (!acceptsInput) return false
        paste(args?.getCharSequence(AccessibilityNodeInfo.ACTION_ARGUMENT_SET_TEXT_CHARSEQUENCE)?.toString().orEmpty())
      }
      else -> return super.performAccessibilityAction(action, args)
    }
    sendAccessibilityEvent(AccessibilityEvent.TYPE_VIEW_SCROLLED)
    return true
  }

  fun reportError(code: Int) { emit("topError", Arguments.createMap().apply {
    putString("terminalHandle", terminalHandle); putInt("code", code)
  }) }

  private fun emit(name: String, data: WritableMap) {
    val react = context as? ReactContext ?: return
    UIManagerHelper.getEventDispatcher(react)?.dispatchEvent(
      TerminalEvent(UIManagerHelper.getSurfaceId(this), id, name, data))
  }
  private class TerminalEvent(surface: Int, tag: Int, private val name: String, private val data: WritableMap) :
    Event<TerminalEvent>(surface, tag) {
    override fun getEventName() = name
    override fun getEventData() = data
    override fun canCoalesce() = false
  }
}
