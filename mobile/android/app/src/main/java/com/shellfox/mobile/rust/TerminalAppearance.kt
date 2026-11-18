package com.shellfox.mobile.rust

import androidx.core.graphics.toColorInt
import org.json.JSONObject

/** Parsed only after sfm_terminal_configure validates the same JSON. */
internal data class TerminalAppearance(
  val background: Int = 0xff171717.toInt(),
  val cursor: Int = 0xffcccccc.toInt(),
  val selectionBackground: Int = 0xff5d88cc.toInt(),
  val selectionForeground: Int? = null,
  val foreground: Int = 0xffcccccc.toInt(),
) {
  companion object {
    fun validated(json: String): TerminalAppearance {
      val value = JSONObject(json)
      fun color(name: String): Int? = if (value.isNull(name) || !value.has(name)) null
        else value.getString(name).toColorInt()
      return TerminalAppearance(requireNotNull(color("background")), color("cursor") ?: requireNotNull(color("foreground")),
        color("selectionBackground") ?: 0xff5d88cc.toInt(), color("selectionForeground"), requireNotNull(color("foreground")))
    }
  }
}
