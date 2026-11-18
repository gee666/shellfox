package com.shellfox.mobile.rust

import org.junit.Assert.*
import org.junit.Test

class TerminalFocusFeedbackTest {
  @Test fun acceptsOnlyOriginalHandleSinkAndInputEpoch() {
    val sink = Any(); val original = FocusActionFence("100", sink, 4)
    assertTrue(original.current("100", sink, 4))
    assertFalse(original.current("101", sink, 4))
    assertFalse(original.current("100", Any(), 4))
    assertFalse(original.current("100", sink, 5))
    assertFalse(original.current("100", null, null))
  }
  @Test fun detachedActionCannotPublishAfterBindingOrDisposal() {
    val original = FocusActionFence("100", null, null)
    assertTrue(original.current("100", null, null))
    assertFalse(original.current("100", Any(), 0))
    assertFalse(original.current("", null, null))
  }
  @Test fun feedbackContainsOnlyTypedObservationsAndDoesNotExecuteFocusOrInput() {
    val result = TerminalFocusFeedback(FocusActionSource.KEYBOARD, true, true, true, true,
      true, FocusRequestResult.ACCEPTED, false, FocusRequestResult.ACCEPTED)
    assertFalse(result.focused)
    assertEquals(FocusRequestResult.ACCEPTED, result.keyboardRequest)
    // Accepted request and actual focus are separate, with no visibility fact.
    assertEquals(listOf("notAttempted", "accepted", "rejected"), FocusRequestResult.values().map { it.wire })
    assertEquals(listOf("keyboard", "surface", "attachment"), FocusActionSource.values().map { it.wire })
  }
}
