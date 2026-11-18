package com.shellfox.mobile.rust

import org.junit.Assert.*
import org.junit.Test
import java.nio.ByteBuffer
import java.nio.ByteOrder

class TerminalFrameTest {
  private fun fixture(text: ByteArray = "界".toByteArray()): ByteArray {
    val out = ByteBuffer.allocate(24 + 40 + text.size).order(ByteOrder.LITTLE_ENDIAN)
    listOf(1, 2, 1, 0, 0, 1).forEach(out::putInt)
    listOf(0xabcdef, 0x123456, 5, 2, text.size).forEach(out::putInt)
    out.put(text)
    listOf(0xabcdef, 0x123456, 0, 0, 0).forEach(out::putInt)
    return out.array()
  }
  @Test fun decodesUtf8WideSpacerColorsAndFlags() {
    val frame = TerminalFrame.decode(fixture())
    assertEquals(2, frame.cols)
    assertEquals("界", frame.text())
    assertEquals(0xffabcdef.toInt(), frame.cells[0].foreground)
    assertEquals(0xff123456.toInt(), frame.cells[0].background)
    assertEquals(5, frame.cells[0].flags)
    assertEquals(2, frame.cells[0].width)
    assertEquals(0, frame.cells[1].width)
  }
  @Test fun rejectsTruncatedInvalidUtf8AndTrailingFrames() {
    for (invalid in listOf(fixture().copyOf(12), fixture(byteArrayOf(0xc0.toByte())), fixture() + byteArrayOf(0))) {
      assertThrows(Exception::class.java) { TerminalFrame.decode(invalid) }
    }
  }
  @Test fun rejectsOversizedGridAndCellLengthBeforeAllocating() {
    val grid = fixture().also { ByteBuffer.wrap(it).order(ByteOrder.LITTLE_ENDIAN).putInt(4, 513) }
    val cell = fixture().also { ByteBuffer.wrap(it).order(ByteOrder.LITTLE_ENDIAN).putInt(40, Int.MAX_VALUE) }
    assertThrows(IllegalArgumentException::class.java) { TerminalFrame.decode(grid) }
    assertThrows(IllegalArgumentException::class.java) { TerminalFrame.decode(cell) }
  }
  @Test fun mailboxReplacesPendingFramesAndKeepsFrontImmutable() {
    val frames = TerminalFrames()
    val first = TerminalFrame.decode(fixture("a".toByteArray()))
    val second = TerminalFrame.decode(fixture("b".toByteArray()))
    val third = TerminalFrame.decode(fixture("c".toByteArray()))
    frames.publish(first)
    assertSame(first, frames.acquire())
    frames.publish(second)
    frames.publish(third)
    assertSame(third, frames.acquire())
    assertEquals("a", first.text())
    assertSame(third, frames.acquire())
    frames.clear()
    assertNull(frames.acquire())
  }
  @Test fun preservesUnsignedNativeIds() {
    assertEquals(-1L, nativeId("18446744073709551615"))
    assertEquals("18446744073709551615", idString(-1L))
    assertTrue(compareNativeIds(0, -1L) < 0)
    assertTrue(compareNativeIds(-1L, Long.MIN_VALUE) > 0)
    assertTrue(compareNativeIds(Long.MAX_VALUE, Long.MIN_VALUE) < 0)
    for (value in listOf("0", "-1", "01", "18446744073709551616"))
      assertThrows(IllegalArgumentException::class.java) { nativeId(value) }
  }
}
