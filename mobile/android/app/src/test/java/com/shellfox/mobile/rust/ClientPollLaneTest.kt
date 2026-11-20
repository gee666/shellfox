package com.shellfox.mobile.rust

import org.junit.Assert.*
import org.junit.Test

class ClientPollLaneTest {
  private class Fixture {
    var time = 0L
    var starts = 0
    var failure: Int? = null
    val lane = ClientPollLane({ time }, {
      failure?.let { throw NativeCallException(it) }
      "poll-${++starts}"
    })
    init { lane.connection(true, "original-binding") }
    fun tick(ready: Boolean = true, ownerDrained: Boolean = true) = lane.advance(ready, ownerDrained)
    fun finish(id: String = "poll-1", success: Boolean = true) = lane.complete(id, 0, success)
  }
  @Test fun multipleViewsShareOnePollUntilOriginalCompletion() {
    val f = Fixture(); repeat(10) { f.tick(listOf(true, true).all { it }) }
    assertEquals(1, f.starts); assertEquals("poll-1", f.lane.poll())
    f.finish("unrelated"); f.tick(); assertEquals(1, f.starts)
    f.finish(); assertEquals(1, f.starts); f.tick(); assertEquals(2, f.starts)
  }
  @Test fun lateActualResultAndOwnerProcessingPrecedeMutationAndNextPoll() {
    val f = Fixture(); f.tick(); var admitted = 0; var completed = false
    f.lane.submit({ admitted++; "attach-original" }) { _, error -> assertNull(error); completed = true }
    assertEquals("poll-1", f.lane.operation); assertEquals(0, admitted)
    repeat(10) { f.tick() }; assertEquals(0, admitted); assertFalse(completed)
    // The original RPC returns real data after the mutation request. It must
    // still reach the original owner before view/context mutation is admitted.
    f.finish(); f.tick(ownerDrained = false)
    assertEquals(0, admitted); assertEquals(1, f.starts)
    f.tick(ready = false, ownerDrained = true)
    assertEquals(1, admitted); assertTrue(completed); assertFalse(f.lane.drained)
    f.tick(); assertEquals(1, f.starts)
    f.finish("attach-original"); f.tick(); assertEquals(2, f.starts)
  }
  @Test fun filesRunDuringPollWithoutOvertakingOrdinaryWork() {
    val f = Fixture(); f.tick()
    var fileStarted = false
    f.lane.submit({ fileStarted = true; "file-1" }, true) { id, error ->
      assertNull(error); assertEquals("file-1", id)
    }
    assertTrue(fileStarted)
    assertEquals("poll-1", f.lane.operation)
    var terminalStarted = false
    f.lane.submit({ terminalStarted = true; "attach" }) { _, error -> assertNull(error) }
    f.finish("file-1")
    var secondFileStarted = false
    f.lane.submit({ secondFileStarted = true; "file-2" }, true) { _, error -> assertNull(error) }
    assertFalse(terminalStarted)
    assertFalse(secondFileStarted)
    f.finish(); f.tick()
    assertTrue(terminalStarted)
    assertFalse(secondFileStarted)
    f.finish("attach"); f.tick()
    assertTrue(secondFileStarted)
  }
  @Test fun cancelledCompletionIsNotADataDrainOrPermissionToMutate() {
    val f = Fixture(); f.tick(); var admitted = false; var failed = false
    f.lane.submit({ admitted = true; null }) { _, error -> failed = error != null }
    f.lane.complete("poll-1", 9, false)
    f.time = 10000; repeat(10) { f.tick() }
    assertFalse(admitted); assertTrue(failed); assertFalse(f.lane.drained)
    assertEquals("poll-1", f.lane.operation); assertEquals(1, f.starts)
  }
  @Test fun v1GenerationFenceCanReconnectAfterAFailedPoll() {
    val f = Fixture(); f.tick()
    f.lane.complete("poll-1", 5, false)
    f.lane.prepareV1Fence()
    f.lane.submit({ "reconnect" }) { id, error -> assertNull(error); assertEquals("reconnect", id) }
    f.tick(); assertEquals(1, f.starts)
    f.lane.complete("reconnect", 0, true)
    f.lane.connection(true, "replacement-binding")
    f.tick(); assertEquals(2, f.starts)
  }
  @Test fun recoveryAndBackgroundPauseNewPollsWithoutDiscardingInflight() {
    val f = Fixture(); f.tick(false); assertEquals(0, f.starts)
    f.tick(); f.tick(false); assertEquals("poll-1", f.lane.operation)
    f.finish(); f.tick(false); assertEquals(1, f.starts)
    f.tick(); assertEquals(2, f.starts)
  }
  @Test fun changedBindingRejectsKnownUnsentMutationWithoutRecertifyingIt() {
    val f = Fixture(); f.tick(); var admitted = false; var rejected = false
    f.lane.submit({ admitted = true; null }) { _, error -> rejected = error is NativeCallException && error.status == 3 }
    f.lane.connection(true, "different-binding"); f.finish(); f.tick(false)
    assertFalse(admitted); assertTrue(rejected)
  }
  @Test fun stopRetainsOriginalOperationUntilNormalCompletionAndNeverPollsAgain() {
    val f = Fixture(); f.tick(); var rejected = false
    f.lane.submit({ fail("retired work ran"); null }) { _, error -> rejected = error != null }
    f.lane.retire(); assertTrue(rejected); assertFalse(f.lane.drained)
    f.time = 100000; repeat(10) { f.tick() }
    assertEquals("poll-1", f.lane.operation); assertEquals(1, f.starts)
    f.finish(); assertTrue(f.lane.drained); f.tick(); assertEquals(1, f.starts)
  }
  @Test fun realPollAdmissionErrorIsNotReadyOrAnEmptySuccess() {
    val f = Fixture(); f.failure = 5
    try { f.tick(); fail("error swallowed") }
    catch (error: NativeCallException) { assertEquals(5, error.status) }
    assertNull(f.lane.operation); assertTrue(f.lane.drained)
  }
  @Test fun actualErrorResponseBacksOffWithoutInventingSuccess() {
    val f = Fixture(); f.tick(); f.finish(success = false)
    f.tick(); assertEquals(1, f.starts)
    f.time = 500; f.tick(); assertEquals(2, f.starts)
  }
}
