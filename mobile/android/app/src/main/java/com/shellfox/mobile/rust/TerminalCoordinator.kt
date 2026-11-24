package com.shellfox.mobile.rust

import android.os.Handler
import android.os.HandlerThread
import org.json.JSONObject
import java.util.concurrent.atomic.AtomicInteger

/** Every thread-affine Rust terminal call, including module calls, runs here. */
internal object TerminalCoordinator {
  private val thread = HandlerThread("ShellfoxRustOwner").apply { start() }
  private val owner = Handler(thread.looper)
  private val queued = AtomicInteger()
  private val terminals = mutableMapOf<String, Entry>()
  private val retiring = mutableMapOf<Any, RetainedModuleRelease>()
  private const val MAX_COMMANDS = 256
  private data class Entry(val handle: Long, val module: Any, var view: TerminalSink? = null,
    var dirty: Boolean = true, var ticking: Boolean = false, var cols: Int = 80, var rows: Int = 24,
    var appearance: TerminalAppearance = TerminalAppearance(), var foreground: Boolean = true, var inputEpoch: Long = 1, var writable: Boolean = false, var recovering: Boolean = false,
    var engineCols: Int = cols, var engineRows: Int = rows,
    var pass: Runnable? = null, var passAt: Long = 0)

  interface TerminalSink {
    fun resetInput(epoch: Long)
    /** A key-bar key or paste: the view finishes the keyboard's word first, then sends it. */
    fun typedInput(json: JSONObject)
    fun userInput()
    fun appearance(appearance: TerminalAppearance)
    fun frame(frame: TerminalFrame)
    fun state(state: String)
    fun error(code: Int)
  }

  fun execute(rejected: () -> Unit, critical: Boolean = false, task: () -> Unit) {
    if (queued.incrementAndGet() > MAX_COMMANDS + (if (critical) 256 else 0)) { queued.decrementAndGet(); rejected(); return }
    owner.post { try { task() } finally { queued.decrementAndGet(); kickReady() } }
  }

  fun create(module: Any, cols: Double, rows: Double, done: (String?, String?) -> Unit) = execute(
    { done(null, "BUSY") },
  ) {
    if (!cols.isFinite() || !rows.isFinite() || cols != cols.toInt().toDouble() || rows != rows.toInt().toDouble() ||
      cols !in 1.0..512.0 || rows !in 1.0..512.0 || cols * rows > 65536) {
      done(null, "INVALID"); return@execute
    }
    try {
      val handle = RustBindings.create(cols.toInt(), rows.toInt())
      val id = idString(handle)
      val entry = Entry(handle, module, cols = cols.toInt(), rows = rows.toInt(), ticking = true)
      terminals[id] = entry
      schedule(id, entry, 0)
      done(id, null)
    } catch (error: NativeCallException) { done(null, nativeCode(error.status)) }
    catch (_: Exception) { done(null, "ENGINE") } catch (_: LinkageError) { done(null, "UNAVAILABLE") }
  }

  fun close(module: Any, id: String, done: (String?) -> Unit) = execute({ done("BUSY") }) {
    val entry = terminals[id]
    if (entry == null || entry.module !== module) { done("STALE"); return@execute }
    ClientTerminalBindings.afterTerminalPoll(id, {
      check(terminals[id] === entry)
      ClientTerminalBindings.unbindTerminal(id)
      RustBindings.close(entry.handle)
      terminals.remove(id)
      entry.view?.state("closed")
    }) { error ->
      when (error) {
        null -> done(null)
        is NativeCallException -> { entry.view?.error(error.status); done(nativeCode(error.status)) }
        else -> { entry.view?.error(5); done("ENGINE") }
      }
    }
  }

  fun release(module: Any) = owner.post {
    if (retiring.containsKey(module)) return@post
    ClientTerminalBindings.release(module)
    retiring[module] = RetainedModuleRelease({ ClientTerminalBindings.released(module) }, {
      terminals.filterValues { it.module === module }.keys.all {
        try { queueDrained(it) } catch (_: Exception) { terminals[it]?.view?.error(5); false }
      }
    }, {
      var closed = true
      terminals.filterValues { it.module === module }.keys.toList().forEach { id ->
        val entry = terminals[id] ?: return@forEach
        try {
          ClientTerminalBindings.unbindTerminal(id)
          RustBindings.close(entry.handle)
          terminals.remove(id); entry.view?.state("closed")
        } catch (_: Exception) { closed = false; entry.view?.error(5) }
      }
      closed
    })
    retireModule(module)
  }
  private fun retireModule(module: Any) {
    val release = retiring[module] ?: return
    if (release.advance()) retiring.remove(module)
    else owner.postDelayed({ retireModule(module) }, 100)
  }

  // Read-only owner progress, not a new ACK or a presentation receipt.
  fun queueDrained(id: String): Boolean {
    val entry = terminals[id] ?: return false
    val progress = RustBindings.progress(entry.handle)
    return progress[0] == progress[1] && progress[2] == 0L && progress[3] == 0L
  }
  fun progress(module: Any, id: String, done: (String?, String?) -> Unit) = execute({ done(null, "BUSY") }) {
    val entry = terminals[id]
    if (entry == null || entry.module !== module) { done(null, "STALE"); return@execute }
    try {
      val p = RustBindings.progress(entry.handle)
      done(JSONObject().put("acceptedSequence", idString(p[0])).put("consumedSequence", idString(p[1]))
        .put("queuedBytes", p[2]).put("queuedChunks", p[3]).toString(), null)
    } catch (error: NativeCallException) { done(null, nativeCode(error.status)) }
    catch (_: Exception) { done(null, "ENGINE") }
  }

  fun attach(id: String, sink: TerminalSink) = execute({ sink.error(1) }) {
    val entry = terminals[id]
    if (entry == null || retiring.containsKey(entry.module)) { sink.error(3); return@execute }
    if (entry.view != null && entry.view !== sink) { sink.error(1); return@execute }
    entry.view = sink
    entry.dirty = true
    sink.resetInput(entry.inputEpoch)
    sink.appearance(entry.appearance)
    sink.state(if (entry.recovering) "recovering" else "ready")
    entry.ticking = true
    schedule(id, entry, 0)
  }

  fun detach(id: String, sink: TerminalSink) {
    owner.post { terminals[id]?.let { if (it.view === sink) it.view = null } }
  }

  // Critical: a size dropped on a full queue would stay wrong until the next layout change.
  fun resize(id: String, sink: TerminalSink, cols: Int, rows: Int, writable: Boolean) = command(id, sink, critical = true) { entry ->
    entry.cols = cols
    entry.rows = rows
    entry.writable = writable
    if (!entry.recovering) {
      flushAccepted(id, entry)
      RustBindings.resize(entry.handle, cols, rows)
      entry.engineCols = cols; entry.engineRows = rows
      ClientTerminalBindings.resize(id, cols, rows, writable)
    }
    entry.dirty = true
  }

  fun input(id: String, sink: TerminalSink, epoch: Long, json: JSONObject) = command(id, sink,
    critical = isRelease(json),
  ) { entry ->
    if (entry.inputEpoch != epoch || entry.recovering) return@command
    flushAccepted(id, entry)
    if (admitInput(id,entry,json)) entry.view?.userInput()
    entry.dirty = true
  }

  fun foreground(module: Any, value: Boolean) {
    owner.post {
      ClientTerminalBindings.foreground(module, value)
      terminals.values.filter { it.module === module }.forEach { it.foreground = value; it.dirty = true }
      kickReady()
    }
  }

  fun later(delay: Long, task: Runnable) { owner.postDelayed(task, delay) }
  fun cancel(task: Runnable) { owner.removeCallbacks(task) }
  /** Rust wake for a client; any thread. */
  fun wakeClient(handle: Long) { owner.post { ClientTerminalBindings.wake(handle) } }
  /** Run a pass now for terminals with a pending frame or queued output. */
  fun kickReady() {
    for ((id, entry) in terminals) {
      val ready = entry.ticking && (entry.dirty && entry.foreground && entry.view != null || try { RustBindings.progress(entry.handle)[3] > 0 } catch (_: Exception) { true })
      if (ready) schedule(id, entry, 0)
    }
  }
  private fun schedule(id: String, entry: Entry, delayMs: Long) {
    val at = android.os.SystemClock.uptimeMillis() + delayMs
    entry.pass?.let { if (entry.passAt <= at) return; owner.removeCallbacks(it) }
    val pass = Runnable { entry.pass = null; tick(id, entry) }
    entry.pass = pass; entry.passAt = at
    owner.postDelayed(pass, delayMs)
  }

  fun configure(module: Any, id: String, json: String) {
    val handle = ownedHandle(module, id)
    require(json.toByteArray().size <= 16384)
    RustBindings.configure(handle, json.toByteArray())
    val entry = requireNotNull(terminals[id])
    entry.appearance = TerminalAppearance.validated(json)
    entry.view?.appearance(entry.appearance)
    entry.dirty = true
  }

  fun typedInput(module: Any, id: String, json: String) {
    val handle = ownedHandle(module, id)
    require(json.toByteArray().size <= 65536)
    val entry = requireNotNull(terminals[id])
    if (entry.recovering) throw NativeInputBusy()
    flushAccepted(id, entry)
    val event = JSONObject(json)
    entry.view?.let { it.typedInput(event); return }
    if (admitInput(id,entry,event)) entry.view?.userInput()
    entry.dirty = true
  }

  fun ownedHandle(module: Any, id: String): Long {
    val entry = terminals[id]
    require(entry != null && entry.module === module)
    return entry.handle
  }

  /** A client bound or released this terminal; input from the old binding is dropped. */
  fun bindingChanged(id: String) {
    terminals[id]?.let { entry ->
      entry.inputEpoch = Math.addExact(entry.inputEpoch, 1)
      entry.view?.resetInput(entry.inputEpoch)
      entry.dirty = true
    }
  }

  fun reset(id: String, hostCols: Int = 0, hostRows: Int = 0) {
    val entry = requireNotNull(terminals[id])
    val hasRecovery = hostCols > 0 && hostRows > 0
    val cols = if (hasRecovery) hostCols else entry.engineCols
    val rows = if (hasRecovery) hostRows else entry.engineRows
    RustBindings.reset(entry.handle, cols, rows)
    entry.engineCols = cols
    entry.engineRows = rows
    entry.recovering = hasRecovery
    entry.inputEpoch = Math.addExact(entry.inputEpoch, 1)
    entry.view?.resetInput(entry.inputEpoch)
    entry.dirty = true
  }

  fun fitAfterRecovery(id: String) {
    val entry = requireNotNull(terminals[id])
    flushAccepted(id, entry)
    RustBindings.resize(entry.handle, entry.cols, entry.rows)
    entry.engineCols = entry.cols; entry.engineRows = entry.rows
    ClientTerminalBindings.resize(id, entry.cols, entry.rows, entry.writable)
    entry.recovering = false
    entry.view?.state("ready")
    entry.dirty = true
  }

  fun scroll(id: String, sink: TerminalSink, rows: Int, x: Float, y: Float, cw: Int, ch: Int, reportMouse: Boolean) = command(id, sink) { entry ->
    if (entry.recovering) return@command
    flushAccepted(id, entry)
    if (reportMouse && RustBindings.mouseTracking(entry.handle)) {
      val event = JSONObject().put("type", "mouse").put("action", "press")
        .put("button", if (rows < 0) "wheelUp" else "wheelDown").put("x", x).put("y", y)
        .put("cellWidth", cw).put("cellHeight", ch).put("anyButtonPressed", false)
      var userInput = false
      repeat(kotlin.math.abs(rows).coerceAtMost(32)) {
        userInput=admitInput(id,entry,event)||userInput
      }
      if (userInput) entry.view?.userInput()
    } else RustBindings.scroll(entry.handle, rows)
    entry.dirty = true
  }

  fun pointer(id: String, sink: TerminalSink, x: Float, y: Float, cw: Int, ch: Int) = command(id, sink) { entry ->
    if (entry.recovering) return@command
    flushAccepted(id, entry)
    if (RustBindings.mouseTracking(entry.handle)) {
      var userInput = false
      for (action in listOf("press", "release")) {
        val event = JSONObject().put("type", "mouse").put("action", action).put("button", "left")
          .put("x", x).put("y", y).put("cellWidth", cw).put("cellHeight", ch)
          .put("anyButtonPressed", action == "press")
        userInput = admitInput(id,entry,event) || userInput
      }
      if (userInput) entry.view?.userInput()
    }
  }

  private fun isRelease(json: JSONObject): Boolean = json.optString("action") == "release" || json.optJSONObject("event")?.optString("action") == "release"
  private fun admitInput(id:String,entry:Entry,event:JSONObject):Boolean {
    val encoded=encode(entry.handle,event.toString()) ?: return false
    ClientTerminalBindings.input(id,encoded,isRelease(event));return encoded.userInitiated
  }
  private fun encode(handle: Long, json: String): EncodedInput? =
    EncodedInput.decode(RustBindings.inputEx(handle, json.toByteArray(Charsets.UTF_8)))

  fun select(id: String, sink: TerminalSink, kind: Int, sx: Int, sy: Int, ex: Int, ey: Int) = command(id, sink) { entry ->
    RustBindings.select(entry.handle, kind, sx, sy, ex, ey)
    entry.dirty = true
  }

  fun copy(id: String, sink: TerminalSink, done: (String) -> Unit) = command(id, sink) { entry ->
    done(RustBindings.copy(entry.handle).toString(Charsets.UTF_8))
  }
  fun link(id: String, sink: TerminalSink, x: Int, y: Int, plaintext: Boolean,
    stillVisible: (() -> Boolean)?, done: (String) -> Unit) = command(id, sink) { entry ->
    // A queued scroll, resize or parser flush may have moved the tapped cell.
    if (stillVisible != null && (entry.dirty || !stillVisible())) return@command
    val bytes = if (plaintext) RustBindings.fileLink(entry.handle, x, y) else RustBindings.hyperlink(entry.handle, x, y)
    done(bytes.toString(Charsets.UTF_8))
  }

  /**
   * Input, a resize or a selection the terminal can't take now is dropped, as
   * 0.9.15 dropped input while it reattached; the view itself keeps working.
   * Only an engine failure is a display error.
   */
  private fun command(id: String, sink: TerminalSink, critical: Boolean = false, task: (Entry) -> Unit) = execute({}, critical) {
    val entry = terminals[id]
    if (entry == null || entry.view !== sink || retiring.containsKey(entry.module)) return@execute
    try { task(entry) }
    catch (_: NativeInputBusy) {}
    catch (_: NativeInputStale) {}
    catch (error: NativeCallException) { if (error.status !in DROPPED_INPUT) sink.error(error.status) }
    catch (_: Exception) { sink.error(5) }
  }
  /** BUSY, INVALID, STALE and EMPTY: the host or the encoder refused this one input. */
  private val DROPPED_INPUT = setOf(1, 2, 3, 7)

  private fun flushAccepted(id: String, entry: Entry) {
    val target = RustBindings.progress(entry.handle)[0]
    // At most 64 chunks were accepted at this barrier. New submissions cannot
    // move its target. Parse old-mode output before encoding input or resizing.
    repeat(64) {
      if (compareNativeIds(RustBindings.progress(entry.handle)[1], target) >= 0) {
        ClientTerminalBindings.consumed(id)
        return
      }
      RustBindings.pump(entry.handle)
      entry.dirty = true
    }
    check(compareNativeIds(RustBindings.progress(entry.handle)[1], target) >= 0)
    ClientTerminalBindings.consumed(id)
  }

  private fun tick(id: String, entry: Entry) {
    if (terminals[id] !== entry) { entry.ticking = false; return }
    val presenting = entry.foreground && entry.view != null
    var queued = false
    try {
      val progress = RustBindings.progress(entry.handle)
      val pumped = progress[3] > 0
      if (presenting && (entry.dirty || pumped)) {
        val frame = TerminalFrame.decode(RustBindings.pumpFrame(entry.handle))
        entry.engineCols = frame.cols
        entry.engineRows = frame.rows
        entry.view?.frame(frame)
        entry.dirty = false
      } else if (progress[3] > 0) {
        RustBindings.pump(entry.handle)
        entry.dirty = true
      }
      ClientTerminalBindings.consumed(id)
      if (pumped) ClientTerminalBindings.terminalProgressed(id)
      queued = RustBindings.progress(entry.handle)[3] > 0
    } catch (_: Exception) {
      entry.view?.error(5)
      ClientTerminalBindings.engineFailed(id)
      entry.view = null
      entry.ticking = false
      return
    }
    // New output and UI changes schedule their own pass. Pace a backlog at
    // frame rate; the idle pass only retries consumption credit.
    schedule(id, entry, when { !queued -> 1000L; presenting -> 16L; else -> 100L })
  }
}
