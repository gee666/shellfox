package com.shellfox.mobile.rust

import java.util.ArrayDeque

/**
 * Owner-thread admission only. Rust captures the witness and owns every ACK.
 * The host long-poll and JS operations run side by side: operations go one at
 * a time in order, and Rust holds output that a poll returns for a terminal
 * whose attachment is still settling.
 */
internal class ClientPollLane(
  private val clock: () -> Long,
  private val startPoll: () -> String,
) {
  private data class Waiting(val epoch: Long, val start: () -> String?, val done: (String?, Exception?) -> Unit)
  private val waiting = ArrayDeque<Waiting>()
  private val mutations = mutableSetOf<String>()
  private var epoch = 0L
  private var authority = ""
  private var connected = false
  private var retired = false
  private var nextPoll = 0L
  private var viewsReady = false
  private var uncertain: Int? = null
  var operation: String? = null
    private set
  val drained: Boolean get() = operation == null && mutations.isEmpty()
  val failedPoll: Boolean get() = uncertain != null

  fun connection(connected: Boolean, authority: String) {
    if (this.authority != authority) { epoch++; this.authority = authority }
    this.connected = connected
  }
  fun submit(start: () -> String?, done: (String?, Exception?) -> Unit) {
    if (retired || uncertain != null) { done(null, NativeCallException(uncertain ?: 3)); return }
    if (waiting.size >= 8) { done(null, NativeCallException(1)); return }
    waiting.addLast(Waiting(epoch, start, done))
  }

  private fun admit(call: Waiting) {
    val id = try { call.start() }
    catch (error: Exception) { call.done(null, error); return }
    if (id != null) mutations.add(id)
    call.done(id, null)
  }
  /**
   * V1 reconnect and disconnect fence the whole native generation, so its
   * outstanding poll and operations can't deliver anything any more. Don't wait
   * for them: their late completions no longer match and are ignored.
   */
  fun prepareV1Fence() {
    operation = null
    mutations.clear()
    uncertain = null
    connected = false
    epoch++
  }
  fun retire() {
    retired = true; connected = false; epoch++
    while (waiting.isNotEmpty()) waiting.removeFirst().done(null, NativeCallException(3))
  }
  fun complete(id: String, status: Int, success: Boolean) {
    if (id == operation) {
      if (status != 0) {
        uncertain = status; connected = false
        while (waiting.isNotEmpty()) waiting.removeFirst().done(null, NativeCallException(status))
        return // Unknown delivery outcome is not a successful drain.
      }
      operation = null
      nextPoll = clock() + if (success) 0 else 500
    }
    mutations.remove(id)
  }
  fun poll(): String {
    uncertain?.let { throw NativeCallException(it) }
    operation?.let { return it }
    if (retired || !connected || !viewsReady) throw NativeInputBusy()
    return startPoll().also { operation = it }
  }
  /** When the owner should look again for a poll that couldn't start; null if none is due. */
  fun retryAt(): Long? = nextPoll.takeIf { !retired && connected && uncertain == null && operation == null }
  fun advance(viewsReady: Boolean, ownerDrained: Boolean) {
    this.viewsReady = viewsReady && ownerDrained
    // Consumed output reaches the parser before an operation can change the views.
    if (!retired && uncertain == null && ownerDrained) {
      while (waiting.isNotEmpty() && mutations.isEmpty()) {
        val call = waiting.removeFirst()
        if (call.epoch != epoch) { call.done(null, NativeCallException(3)); continue }
        admit(call)
      }
    }
    if (!retired && connected && this.viewsReady && operation == null && clock() >= nextPoll) {
      try { poll() }
      catch (_: NativeInputBusy) { nextPoll = clock() + 100 }
      catch (error: NativeCallException) {
        if (error.status != 1) throw error
        nextPoll = clock() + 100
      }
    }
  }
}
