package com.shellfox.mobile.rust

import android.graphics.Canvas
import android.graphics.Paint
import android.graphics.RectF
import android.graphics.Typeface
import androidx.core.graphics.withClip

internal class TerminalRenderer(density: Float) {
  var appearance = TerminalAppearance()
  private val paint = Paint(Paint.ANTI_ALIAS_FLAG or Paint.SUBPIXEL_TEXT_FLAG).apply {
    typeface = Typeface.MONOSPACE
    textSize = 14f * density
  }
  val cellWidth: Float get() = paint.measureText("M").coerceAtLeast(1f)
  val cellHeight: Float get() = (paint.fontMetrics.descent - paint.fontMetrics.ascent).coerceAtLeast(1f)
  val baseline: Float get() = -paint.fontMetrics.ascent

  fun fontSize(sp: Float, metrics: android.util.DisplayMetrics) {
    paint.textSize = android.util.TypedValue.applyDimension(android.util.TypedValue.COMPLEX_UNIT_SP, sp.coerceIn(6f, 48f), metrics)
  }

  fun draw(canvas: Canvas, frame: TerminalFrame, selection: IntRange?, preedit: String) {
    val width = cellWidth
    val height = cellHeight
    canvas.drawColor(appearance.background)
    frame.cells.forEachIndexed { index, cell ->
      val x = index % frame.cols * width
      val y = index / frame.cols * height
      val inverse = cell.flags and 8 != 0
      paint.color = if (inverse) cell.foreground else cell.background
      canvas.drawRect(x, y, x + width, y + height, paint)
      if (selection?.contains(index) == true || cell.flags and 128 != 0) {
        paint.color = appearance.selectionBackground
        canvas.drawRect(x, y, x + width, y + height, paint)
      }
    }
    for (row in 0 until frame.rows) drawRow(canvas, frame, row, width, height)
    if (frame.cursorX < frame.cols && frame.cursorY < frame.rows) {
      val x = frame.cursorX * width
      val y = frame.cursorY * height
      if (frame.cursorVisible) {
        paint.color = appearance.cursor
        paint.alpha = 160
        canvas.drawRect(RectF(x, y, x + width, y + height), paint)
        paint.alpha = 255
      }
      if (preedit.isNotEmpty()) {
        paint.color = appearance.foreground
        paint.isUnderlineText = true
        canvas.drawText(preedit, x, y + baseline, paint)
        paint.isUnderlineText = false
      }
    }
  }

  private fun drawRow(canvas: Canvas, frame: TerminalFrame, row: Int, width: Float, height: Float) {
    val cells = frame.cells.subList(row * frame.cols, (row + 1) * frame.cols)
    val offsets = IntArray(cells.size + 1)
    val text = buildString {
      cells.forEachIndexed { index, cell ->
        offsets[index] = length
        if (cell.width != 0) append(cell.text.ifEmpty { " " })
      }
      offsets[cells.size] = length
    }.toCharArray()
    var start = 0
    while (start < cells.size) {
      val cell = cells[start]
      var end = start + 1
      while (end < cells.size && cells[end].flags == cell.flags && cells[end].foreground == cell.foreground &&
        cells[end].background == cell.background) end++
      if (cell.flags and 32 == 0 && offsets[end] > offsets[start]) {
        val count = offsets[end] - offsets[start]
        paint.color = if (cell.flags and 128 != 0 && appearance.selectionForeground != null)
          requireNotNull(appearance.selectionForeground) else if (cell.flags and 8 != 0) cell.background else cell.foreground
        paint.alpha = if (cell.flags and 16 != 0) 128 else 255
        paint.typeface = Typeface.create(Typeface.MONOSPACE, when (cell.flags and 3) {
          3 -> Typeface.BOLD_ITALIC
          1 -> Typeface.BOLD
          2 -> Typeface.ITALIC
          else -> Typeface.NORMAL
        })
        paint.isUnderlineText = cell.flags and 4 != 0
        paint.isStrikeThruText = cell.flags and 64 != 0
        val x = start * width
        val y = row * height
        canvas.withClip(x, y, end * width, y + height) {
          // Shape each style run using full-row context and system font fallback.
          drawTextRun(text, offsets[start], count, 0, text.size, x, y + baseline, false, paint)
        }
      }
      start = end
    }
    paint.alpha = 255
    paint.isUnderlineText = false
    paint.isStrikeThruText = false
    paint.typeface = Typeface.MONOSPACE
  }
}
