package com.shellfox.mobile.rust

import org.junit.Assert.*
import org.junit.Test

class InputRefreshGateTest {
  private class Fixture {
    val gate = InputRefreshGate()
    val sink = Any()
    var state: InputRefreshState? = InputRefreshState("100", sink, 1, Any(), true, true, true, true, true)
    val queued = mutableListOf<() -> Unit>()
    var calls = 0
    var fail = false
    var queueAccepted = true
    var readFailure = false
    var postFailure = false
    fun update() = gate.update(state, { task ->
      if (postFailure) throw IllegalStateException("Unknown scheduling")
      if (queueAccepted) queued.add(task); queueAccepted
    }, {
      if (readFailure) throw IllegalStateException("Unknown current context")
      state
    }) {
      calls++; if (fail) throw IllegalStateException("Unknown IMM outcome")
    }
    fun run() { while (queued.isNotEmpty()) queued.removeAt(0)() }
    fun change(work: (InputRefreshState) -> InputRefreshState) { state = work(requireNotNull(state)) }
  }
  @Test fun firstFrameWhileReadOnlyDoesNotConsumeReenable() {
    val f = Fixture(); f.change { it.copy(inputAllowed = false) }; f.update(); f.run(); assertEquals(0, f.calls)
    f.change { it.copy(inputAllowed = true) }; f.update(); f.run(); assertEquals(1, f.calls)
  }
  @Test fun stalePostedEpochCannotRefresh() {
    val f = Fixture(); f.update(); f.change { it.copy(epoch = 2) }; f.run(); assertEquals(0, f.calls)
    f.update(); f.run(); assertEquals(1, f.calls)
  }
  @Test fun clearedFrameAndSameEpochResetCannotRefreshOldPost() {
    val f = Fixture(); f.update(); f.change { it.copy(frameAvailable = false) }; f.run(); assertEquals(0, f.calls)
    f.change { it.copy(frameAvailable = true) }; f.update(); f.change { it.copy(reset = Any()) }; f.run(); assertEquals(0, f.calls)
    f.update(); f.run(); assertEquals(1, f.calls)
  }
  @Test fun sinkHandleAndDetachInvalidateOriginalPost() {
    for (kind in 0..2) {
      val f = Fixture(); f.update()
      when (kind) { 0 -> f.change { it.copy(sink = Any()) }; 1 -> f.change { it.copy(handle = "101") }; else -> f.state = null }
      f.run(); assertEquals(0, f.calls)
    }
  }
  @Test fun frameAttachmentWindowFocusAndInputEligibilityAreAllRequired() {
    for (kind in 0..4) {
      val f = Fixture(); f.update()
      f.change { when (kind) {
        0 -> it.copy(frameAvailable = false)
        1 -> it.copy(attached = false)
        2 -> it.copy(windowFocused = false)
        3 -> it.copy(focused = false)
        else -> it.copy(inputAllowed = false)
      } }
      f.run(); assertEquals(0, f.calls)
    }
  }
  @Test fun repeatedEligibleFramesDeduplicateButActualReenableCanRefresh() {
    val f = Fixture(); repeat(3) { f.update() }; f.run(); repeat(3) { f.update() }; f.run(); assertEquals(1, f.calls)
    f.change { it.copy(inputAllowed = false) }; f.update()
    f.change { it.copy(inputAllowed = true) }; repeat(3) { f.update() }; f.run(); assertEquals(2, f.calls)
  }
  @Test fun explicitResetInvalidationRejectsEvenEqualLookingContext() {
    val f = Fixture(); f.update(); f.gate.invalidate(); f.run(); assertEquals(0, f.calls)
    f.update(); f.run(); assertEquals(1, f.calls)
  }
  @Test fun unknownAttemptIsNotRetriedForTheSameEpisode() {
    val f = Fixture(); f.fail = true; f.update(); f.run(); assertEquals(1, f.calls)
    f.fail = false; repeat(3) { f.update() }; f.run(); assertEquals(1, f.calls)
    f.change { it.copy(inputAllowed = false) }; f.update()
    f.change { it.copy(inputAllowed = true) }; f.update(); f.run(); assertEquals(2, f.calls)
  }
  @Test fun unknownSchedulingIsNotReportedAsSuccessOrRetried() {
    val f = Fixture(); f.queueAccepted = false; f.update(); f.queueAccepted = true; f.update(); f.run(); assertEquals(0, f.calls)
    f.gate.invalidate(); f.update(); f.run(); assertEquals(1, f.calls)
  }
  @Test fun uncertainContextReadOrPostIsRetainedWithoutRetry() {
    for (kind in 0..1) {
      val f = Fixture(); f.readFailure = kind == 0; f.postFailure = kind == 1
      f.update(); f.run(); assertEquals(0, f.calls)
      f.readFailure = false; f.postFailure = false; f.update(); f.run(); assertEquals(0, f.calls)
      f.gate.invalidate(); f.update(); f.run(); assertEquals(1, f.calls)
    }
  }
  @Test fun queuedOldEpisodeCannotInvalidateANewerEpisode() {
    val f = Fixture(); f.update(); f.change { it.copy(epoch = 2, reset = Any()) }; f.update(); f.run(); assertEquals(1, f.calls)
  }
}
