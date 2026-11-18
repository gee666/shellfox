package com.shellfox.mobile.rust

import org.junit.Assert.*
import org.junit.Test

class NativeInputAdmissionTest {
  private fun input(text: String, user: Boolean = true) = EncodedInput(text.toByteArray(), user)

  @Test fun busyWriteResizeAndReleaseRetainPreAdmissionIntentOrder() {
    var busy = true
    val accepted = mutableListOf<String>()
    val hints = mutableListOf<Boolean>()
    val admission = NativeInputAdmission(
      { bytes, hint -> if (busy) 1 else { accepted.add(bytes.toString(Charsets.UTF_8)); hints.add(hint); 0 } },
      { cols, rows -> if (busy) 1 else { accepted.add("resize:$cols:$rows"); 0 } },
    )
    admission.submit(input("press"), false)
    admission.submitResize(100, 30)
    admission.submit(input("release", false), true)
    admission.drain()
    assertTrue(accepted.isEmpty())
    busy = false
    admission.drain()
    assertEquals(listOf("press", "resize:100:30", "release"), accepted)
    assertEquals(listOf(true, false), hints)
  }
  @Test fun reservedReleaseCapacityAndCopiedBytesSurviveOrdinarySaturation() {
    var busy = true
    val accepted = mutableListOf<String>()
    val admission = NativeInputAdmission(
      { bytes, _ -> if (busy) 1 else { accepted.add(bytes.toString(Charsets.UTF_8)); 0 } }, { _, _ -> 0 },
    )
    val mutable = input("a")
    admission.submit(mutable, false)
    mutable.bytes[0] = 'x'.code.toByte()
    repeat(63) { admission.submit(input("b"), false) }
    assertThrows(NativeInputBusy::class.java) { admission.submit(input("rejected"), false) }
    admission.submit(input("release", false), true)
    busy = false
    admission.drain()
    assertEquals(65, accepted.size)
    assertEquals("a", accepted.first())
    assertEquals("release", accepted.last())
  }
  @Test fun staleLeaseClearsPendingInputInsteadOfReplayingIntoNewGeneration() {
    var status = 1
    val accepted = mutableListOf<String>()
    val admission = NativeInputAdmission(
      { bytes, _ -> if (status == 0) accepted.add(bytes.toString(Charsets.UTF_8)); status }, { _, _ -> status },
    )
    admission.submit(input("old"), false)
    status = 3
    admission.drain()
    status = 0
    admission.drain()
    admission.submit(input("new"), false)
    assertEquals(listOf("new"), accepted)
  }
  @Test fun layoutCoalescingNeverCrossesAnInputAndReadonlyDropsOnlyPendingResizes() {
    var busy = true
    val accepted = mutableListOf<String>()
    val admission = NativeInputAdmission(
      { bytes, _ -> if (busy) 1 else { accepted.add(bytes.toString(Charsets.UTF_8)); 0 } },
      { cols, _ -> if (busy) 1 else { accepted.add("resize:$cols"); 0 } },
    )
    admission.submitResize(80, 24)
    admission.submitResize(90, 30)
    admission.submit(input("key"), false)
    admission.submitResize(100, 30)
    busy = false
    admission.drain()
    assertEquals(listOf("resize:90", "key", "resize:100"), accepted)
    accepted.clear(); busy = true
    admission.submit(input("press"), false)
    admission.submitResize(120, 30)
    admission.submit(input("release", false), true)
    admission.dropResizes()
    busy = false
    admission.drain()
    assertEquals(listOf("press", "release"), accepted)
  }
  @Test fun retriesPreserveRustHintAndNulBytesIndependentlyOfReleasePriority() {
    var busy = true
    val accepted = mutableListOf<EncodedInput>()
    val admission = NativeInputAdmission(
      { bytes, hint -> if (busy) 1 else { accepted.add(EncodedInput(bytes.copyOf(), hint)); 0 } }, { _, _ -> 0 },
    )
    val passive = requireNotNull(EncodedInput.decode(byteArrayOf(0, 0, 0x1b, 0)))
    admission.submit(passive, false)
    admission.submit(input("mouse release", true), true)
    busy = false
    admission.drain()
    assertFalse(accepted[0].userInitiated)
    assertArrayEquals(byteArrayOf(0, 0x1b, 0), accepted[0].bytes)
    assertTrue(accepted[1].userInitiated)
    assertNull(EncodedInput.decode(byteArrayOf()))
    for (bad in listOf(byteArrayOf(1), byteArrayOf(2, 1), ByteArray(65538)))
      assertThrows(IllegalArgumentException::class.java) { EncodedInput.decode(bad) }
  }
}
