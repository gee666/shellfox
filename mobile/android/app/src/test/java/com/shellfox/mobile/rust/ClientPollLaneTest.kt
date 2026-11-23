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
  @Test fun retryIsDueOnlyWhenAPollCouldNotStart() {
    val f = Fixture(); f.tick()
    assertNull(f.lane.retryAt()) // the running long-poll completes with a Rust wake
    f.time = 100; f.finish(success = false)
    assertEquals(600L, f.lane.retryAt())
    f.time = 600; f.tick(); assertEquals(2, f.starts); assertNull(f.lane.retryAt())
  }
  @Test fun multipleViewsShareOnePollUntilOriginalCompletion() {
    val f = Fixture(); repeat(10) { f.tick(listOf(true, true).all { it }) }
    assertEquals(1, f.starts); assertEquals("poll-1", f.lane.poll())
    f.finish("unrelated"); f.tick(); assertEquals(1, f.starts)
    f.finish(); assertEquals(1, f.starts); f.tick(); assertEquals(2, f.starts)
  }
  @Test fun operationsRunAlongsideTheLongPollOneAtATimeInOrder() {
    val f = Fixture(); f.tick(); val started = mutableListOf<String>()
    f.lane.submit({ started += "attach"; "attach" }) { _, error -> assertNull(error) }
    f.lane.submit({ started += "file"; "file" }) { _, error -> assertNull(error) }
    f.tick()
    assertEquals(listOf("attach"), started); assertEquals("poll-1", f.lane.operation)
    f.finish(); f.tick(); assertEquals(2, f.starts) // the next poll does not wait for the operation
    f.finish("attach"); f.tick(); assertEquals(listOf("attach", "file"), started)
  }
  @Test fun consumedOutputReachesTheParserBeforeAnOperationIsAdmitted() {
    val f = Fixture(); f.tick(); var admitted = false
    f.lane.submit({ admitted = true; "attach" }) { _, error -> assertNull(error) }
    f.tick(ownerDrained = false); assertFalse(admitted)
    f.tick(); assertTrue(admitted)
  }
  @Test fun reconnectDoesNotWaitForTheFencedLongPoll() {
    val f = Fixture(); f.tick(); var reconnected = false
    f.lane.prepareV1Fence()
    f.lane.submit({ reconnected = true; "reconnect" }) { _, error -> assertNull(error) }
    f.tick(); assertTrue(reconnected); assertTrue(f.lane.operation == null)
    f.lane.complete("poll-1", 3, false) // the fenced poll's late result
    assertFalse(f.lane.failedPoll)
    f.lane.complete("reconnect", 0, true); f.lane.connection(true, "replacement-binding")
    f.tick(); assertEquals(2, f.starts)
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
