package com.shellfox.mobile.rust

/** Local IMM context only. This is not parser readiness or an input/lease grant. */
internal data class InputRefreshState(
  val handle: String, val sink: Any, val epoch: Long, val reset: Any,
  val frameAvailable: Boolean, val attached: Boolean, val windowFocused: Boolean,
  val focused: Boolean, val inputAllowed: Boolean,
) {
  val eligible get() = handle.isNotEmpty() && epoch > 0 && frameAvailable && attached && windowFocused && focused && inputAllowed
  fun sameContext(other: InputRefreshState) = handle == other.handle && sink === other.sink && epoch == other.epoch && reset === other.reset
}

/** UI-thread only. Actual invalidation starts a new eligibility episode, not a retry. */
internal class InputRefreshGate {
  private enum class Outcome { QUEUED, ATTEMPTED, UNKNOWN }
  private class Episode(val original: InputRefreshState, var outcome: Outcome? = null)
  private var episode: Episode? = null
  // Retain the last uncertain context even after an actual eligibility loss.
  private var lastUnknown: Episode? = null

  fun invalidate() { episode = null }

  fun update(state: InputRefreshState?, post: (() -> Unit) -> Boolean,
    current: () -> InputRefreshState?, restart: () -> Unit) {
    if (state?.eligible != true) { invalidate(); return }
    val original = episode?.takeIf { it.original.sameContext(state) }
      ?: Episode(state).also { episode = it }
    if (original.outcome != null) return
    original.outcome = Outcome.QUEUED
    try {
      val queued = post {
        if (episode !== original) return@post
        try {
          val now = current()
          if (now?.eligible != true || !original.original.sameContext(now)) { invalidate(); return@post }
          // The void call is attempted, never an input-connection or SDK receipt.
          original.outcome = Outcome.ATTEMPTED
          restart()
        } catch (_: RuntimeException) {
          original.outcome = Outcome.UNKNOWN; lastUnknown = original
        }
      }
      if (!queued) { original.outcome = Outcome.UNKNOWN; lastUnknown = original }
    } catch (_: RuntimeException) {
      original.outcome = Outcome.UNKNOWN; lastUnknown = original
    }
  }
}
