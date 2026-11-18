package com.shellfox.mobile.rust

import org.junit.Assert.*
import org.junit.Test

class ClientRetirementTest {
  private class Fixture {
    val poll = ClientPollLane({ 0L }, { "original-poll" })
    val close = ClientCloseGate()
    var consumed = true
    var clientRemoved = false
    var nativeCloses = 0
    var unbinds = 0
    var engineCloses = 0
    var reports = 0
    val lifetime = RetainedModuleRelease({ clientRemoved }, { consumed }, {
      unbinds++; engineCloses++; true
    })
    init { poll.connection(true, "original"); poll.advance(true, true); poll.retire() }
    fun reportDeadlineOrError() { reports++ }
    fun finishClient() {
      if (close.finish(poll.drained, consumed) { nativeCloses++ }) clientRemoved = true
    }
  }
  @Test fun invalidateDeadlineReportDoesNotAuthorizeUnbindOrEngineClose() {
    val f = Fixture(); f.reportDeadlineOrError()
    repeat(10) { f.finishClient(); assertFalse(f.lifetime.advance()) }
    assertEquals(1, f.reports); assertEquals(0, f.nativeCloses)
    assertEquals(0, f.unbinds); assertEquals(0, f.engineCloses)
    assertFalse(f.clientRemoved); assertEquals("original-poll", f.poll.operation)
  }
  @Test fun lateNormalResultThenOwnerConsumptionThenNativeCloseAllowsSingleCleanup() {
    val f = Fixture(); f.reportDeadlineOrError()
    f.poll.complete("original-poll", 0, true); f.consumed = false
    f.close.disconnectCompleted(true)
    f.finishClient(); assertFalse(f.lifetime.advance()); assertEquals(0, f.nativeCloses)
    f.consumed = true; f.finishClient(); assertTrue(f.lifetime.advance())
    repeat(5) { f.finishClient(); assertTrue(f.lifetime.advance()) }
    assertEquals(1, f.nativeCloses); assertEquals(1, f.unbinds); assertEquals(1, f.engineCloses)
  }
  @Test fun cancelledOrErrorPollCompletionKeepsInvalidatedLifetimeOwned() {
    for (status in listOf(9, 10, 11)) {
      val f = Fixture(); f.poll.complete("original-poll", status, false)
      f.reportDeadlineOrError(); f.close.disconnectCompleted(true)
      f.finishClient(); assertFalse(f.lifetime.advance())
      assertFalse(f.poll.drained); assertFalse(f.clientRemoved)
      assertEquals(0, f.nativeCloses); assertEquals(0, f.unbinds); assertEquals(0, f.engineCloses)
    }
  }
  @Test fun failedDisconnectDoesNotBecomeNativeClosePermissionAtDeadline() {
    val f = Fixture(); f.poll.complete("original-poll", 0, true)
    f.close.disconnectCompleted(false); f.reportDeadlineOrError()
    f.finishClient(); assertFalse(f.lifetime.advance())
    assertEquals(0, f.nativeCloses); assertEquals(0, f.engineCloses)
  }
  @Test fun nativeCloseErrorRetainsClientAndNeverRetriesUnknownClose() {
    val gate = ClientCloseGate(); gate.disconnectCompleted(true); var attempts = 0
    try { gate.finish(true, true) { attempts++; throw NativeCallException(5) }; fail("error hidden") }
    catch (error: NativeCallException) { assertEquals(5, error.status) }
    assertFalse(gate.closed)
    assertFalse(gate.finish(true, true) { attempts++ }); assertEquals(1, attempts)
    var engineCloses = 0
    val lifetime = RetainedModuleRelease({ gate.closed }, { true }, { engineCloses++; true })
    assertFalse(lifetime.advance()); assertEquals(0, engineCloses)
  }
  @Test fun queuedUnconsumedWorkRetainsEnginesEvenAfterClientCompletion() {
    var consumed = false; var closes = 0
    val lifetime = RetainedModuleRelease({ true }, { consumed }, { closes++; true })
    assertFalse(lifetime.advance()); assertEquals(0, closes)
    consumed = true; assertTrue(lifetime.advance()); assertTrue(lifetime.advance()); assertEquals(1, closes)
  }
  @Test fun uncertainEngineCloseIsRetainedAndNotReplayedAsCleanupSuccess() {
    var attempts = 0
    val lifetime = RetainedModuleRelease({ true }, { true }, { attempts++; false })
    repeat(5) { assertFalse(lifetime.advance()) }; assertEquals(1, attempts)
  }
}
