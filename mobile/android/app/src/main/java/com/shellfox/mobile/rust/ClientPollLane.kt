package com.shellfox.mobile.rust

import java.util.ArrayDeque

/** Owner-thread admission only. Rust captures the witness and owns every ACK. */
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

  fun connection(connected: Boolean, authority: String) {
    if (this.authority != authority) { epoch++; this.authority = authority }
    this.connected = connected
  }
  fun submit(start: () -> String?, done: (String?, Exception?) -> Unit) {
    if (retired || uncertain != null) { done(null, NativeCallException(uncertain ?: 3)); return }
    if (waiting.size >= 8) { done(null, NativeCallException(1)); return }
    waiting.addLast(Waiting(epoch, start, done))
    // Admission waits for the original result and owner processing. Canceling
    // an SDK operation would drop its future, not drain its delivery result.
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
    if (retired || !connected || !viewsReady || waiting.isNotEmpty() || mutations.isNotEmpty()) throw NativeInputBusy()
    return startPoll().also { operation = it }
  }
  fun advance(viewsReady: Boolean, ownerDrained: Boolean) {
    this.viewsReady = viewsReady && ownerDrained
    if (!retired && drained && ownerDrained) {
      while (waiting.isNotEmpty() && drained) {
        val call = waiting.removeFirst()
        if (call.epoch != epoch) { call.done(null, NativeCallException(3)); continue }
        val id = try { call.start() }
        catch (error: Exception) { call.done(null, error); continue }
        if (id != null) mutations.add(id)
        call.done(id, null)
      }
    }
    if (!retired && connected && viewsReady && ownerDrained && drained && waiting.isEmpty() && clock() >= nextPoll) {
      try { poll() }
      catch (_: NativeInputBusy) { nextPoll = clock() + 100 }
      catch (error: NativeCallException) {
        if (error.status != 1) throw error
        nextPoll = clock() + 100
      }
    }
  }
}
