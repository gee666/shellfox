package com.shellfox.mobile.rust

import java.nio.ByteBuffer
import java.nio.ByteOrder
import java.nio.charset.CodingErrorAction

internal data class TerminalCell(val text: String, val foreground: Int, val background: Int, val flags: Int, val width: Int)
internal data class TerminalFrame(
  val cols: Int, val rows: Int, val cursorX: Int, val cursorY: Int,
  val cursorVisible: Boolean, val cells: List<TerminalCell>,
) {
  fun text(): String = (0 until rows).joinToString("\n") { row ->
    cells.subList(row * cols, (row + 1) * cols).joinToString("") { if (it.width == 0) "" else it.text.ifEmpty { " " } }.trimEnd()
  }

  fun fileLinkRow(x: Int, y: Int): Pair<String, Int>? {
    val row = cells.subList(y * cols, (y + 1) * cols)
    val text = StringBuilder()
    var offset = 0
    var lastGlyph = 0
    row.forEachIndexed { column, cell ->
      if (cell.width != 0) {
        lastGlyph = text.length
        text.append(cell.text.ifEmpty { " " })
      }
      if (column == x) offset = lastGlyph
      if (text.length > 8192) return null
    }
    return text.toString().trimEnd() to offset
  }

  companion object {
    fun decode(bytes: ByteArray): TerminalFrame {
      require(bytes.size in 24..16 * 1024 * 1024)
      val buffer = ByteBuffer.wrap(bytes).order(ByteOrder.LITTLE_ENDIAN)
      require(buffer.int == 1)
      val cols = buffer.int
      val rows = buffer.int
      require(cols in 1..512 && rows in 1..512 && cols * rows <= 65536)
      val x = buffer.int
      val y = buffer.int
      val visible = buffer.int != 0
      val decoder = Charsets.UTF_8.newDecoder().onMalformedInput(CodingErrorAction.REPORT)
      val cells = List(cols * rows) {
        require(buffer.remaining() >= 20)
        val foreground = buffer.int or (0xff shl 24)
        val background = buffer.int or (0xff shl 24)
        val flags = buffer.int
        val width = buffer.int
        val length = buffer.int
        require(width in 0..2 && length in 0..65536 && length <= buffer.remaining())
        val text = buffer.slice().apply { limit(length) }
        buffer.position(buffer.position() + length)
        TerminalCell(decoder.decode(text).toString(), foreground, background, flags, width)
      }
      require(!buffer.hasRemaining())
      return TerminalFrame(cols, rows, x, y, visible, cells)
    }
  }
}

/** One immutable front frame and one replaceable pending frame. Never queue UI frames. */
internal class TerminalFrames {
  private var pending: TerminalFrame? = null
  private var front: TerminalFrame? = null
  @Synchronized fun publish(frame: TerminalFrame) { pending = frame }
  @Synchronized fun acquire(): TerminalFrame? {
    pending?.let { front = it; pending = null }
    return front
  }
  @Synchronized fun clear() { pending = null; front = null }
}
