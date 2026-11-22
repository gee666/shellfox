package com.shellfox.mobile.rust

import java.util.ArrayDeque

internal class NativeInputBusy : RuntimeException()
internal class NativeInputStale : RuntimeException()

/** Retains only intents Rust has NOT accepted. Rust owns the accepted FIFO. */
internal class NativeInputAdmission(
  private val write: (ByteArray, Boolean) -> Int,
  private val resize: (Int, Int) -> Int,
) {
  private sealed interface Intent { val release: Boolean; val cost: Int }
  private data class Input(val bytes: ByteArray, val userInitiated: Boolean, override val release: Boolean) : Intent {
    override val cost get() = bytes.size
  }
  private data class Resize(val cols: Int, val rows: Int) : Intent {
    override val release = false
    override val cost = 16
  }
  private val pending = ArrayDeque<Intent>()
  private var bytes = 0
  private var ordinary = 0
  val hasPending: Boolean get() = pending.isNotEmpty()

  fun submit(value: EncodedInput, release: Boolean) {
    if (!offer(Input(value.bytes.copyOf(), value.userInitiated, release))) throw NativeInputStale()
  }
  fun submitNativeJson(value: String, release: Boolean) {
    val payload = value.toByteArray(Charsets.UTF_8)
    require(payload.size <= 1_048_576)
    if (!offer(Input(payload, true, release))) throw NativeInputStale()
  }
  fun submitResize(cols: Int, rows: Int): Boolean {
    // Coalesce consecutive layout changes only, never across a key intent.
    if (pending.peekLast() is Resize) removeLast()
    return offer(Resize(cols, rows))
  }
  private fun execute(intent: Intent): Int = when (intent) {
    is Input -> write(intent.bytes, intent.userInitiated)
    is Resize -> resize(intent.cols, intent.rows)
  }
  private fun offer(intent: Intent): Boolean {
    if (pending.isEmpty()) when (val status = execute(intent)) {
      0 -> return true
      1 -> Unit
      3, 7 -> return false
      else -> throw NativeCallException(status)
    }
    // Dedicated release capacity prevents ordinary input from filling every
    // slot. Later input/resize cannot overtake a rejected hardware release.
    if (pending.size >= 320 || bytes + intent.cost > 196608 ||
      (!intent.release && (ordinary >= 64 || bytes + intent.cost > 131072))) throw NativeInputBusy()
    pending.addLast(intent)
    bytes += intent.cost
    if (!intent.release) ordinary++
    return true
  }
  fun drain() {
    while (pending.isNotEmpty()) {
      val next = pending.first
      when (val status = execute(next)) {
        0 -> { pending.removeFirst(); removed(next) }
        1 -> return
        3, 7 -> { clear(); return } // Lease was fenced; never replay into a new generation.
        else -> throw NativeCallException(status)
      }
    }
  }
  fun dropResizes() {
    val iterator = pending.iterator()
    while (iterator.hasNext()) {
      val intent = iterator.next()
      if (intent is Resize) { iterator.remove(); removed(intent) }
    }
  }
  private fun removeLast() { removed(pending.removeLast()) }
  private fun removed(intent: Intent) { bytes -= intent.cost; if (!intent.release) ordinary-- }
  fun clear() { pending.clear(); bytes = 0; ordinary = 0 }
}
