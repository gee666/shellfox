package com.shellfox.mobile.rust

import android.view.KeyEvent
import org.json.JSONObject

/** Actual Android key-code vocabulary, not a Unicode-to-physical-key guess. */
internal object ManagerKeyInput {
  fun physical(code: Int): Int? = when (code) {
    in KeyEvent.KEYCODE_A..KeyEvent.KEYCODE_Z -> 20 + code - KeyEvent.KEYCODE_A
    in KeyEvent.KEYCODE_0..KeyEvent.KEYCODE_9 -> 6 + code - KeyEvent.KEYCODE_0
    in KeyEvent.KEYCODE_F1..KeyEvent.KEYCODE_F12 -> 121 + code - KeyEvent.KEYCODE_F1
    in KeyEvent.KEYCODE_NUMPAD_0..KeyEvent.KEYCODE_NUMPAD_9 -> 80 + code - KeyEvent.KEYCODE_NUMPAD_0
    else -> named[code]
  }
  fun event(event: KeyEvent, logical: Int, unshifted: Int, sticky: Int, action: String): JSONObject? {
    val physical = physical(event.keyCode) ?: return null
    val meta = event.metaState
    val modifiers = sticky or (if (event.isShiftPressed) 1 else 0) or
      (if (event.isCtrlPressed) 2 else 0) or (if (event.isAltPressed) 4 else 0) or
      (if (event.isMetaPressed) 8 else 0) or (if (event.isCapsLockOn) 16 else 0) or
      (if (event.isNumLockOn) 32 else 0) or
      (if (meta and KeyEvent.META_SHIFT_RIGHT_ON != 0) 64 else 0) or
      (if (meta and KeyEvent.META_CTRL_RIGHT_ON != 0) 128 else 0) or
      (if (meta and KeyEvent.META_ALT_RIGHT_ON != 0) 256 else 0) or
      (if (meta and KeyEvent.META_META_RIGHT_ON != 0) 512 else 0)
    val text = if (action != "release" && printable(logical)) String(Character.toChars(logical)) else ""
    // Android exposes no consumed-modifier mask. Preserve supplied state with
    // consumed=0; never manufacture a side/subset normalization.
    return ManagerKeyInput.logical(physical, action, modifiers, text, if (printable(unshifted)) unshifted else null)
  }
  fun logical(physical: Int = 0, action: String = "press", modifiers: Int, text: String, unshifted: Int? = null): JSONObject =
    JSONObject().put("kind", "key").put("event", JSONObject().put("physical", physical)
      .put("action", action).put("modifiers", modifiers).put("consumedModifiers", 0)
      .put("text", text).put("unshiftedCodepoint", unshifted ?: JSONObject.NULL).put("composing", false))
  fun release(value: JSONObject): JSONObject = JSONObject(value.toString()).also {
    val event = it.optJSONObject("event")
    if (it.optString("kind") == "key" && event != null) { event.put("action", "release"); event.put("text", "") }
    else { it.put("action", "release"); it.remove("text") }
  }
  private fun printable(value: Int): Boolean = Character.isValidCodePoint(value) && value >= 32 && value != 127 &&
    value !in 0xd800..0xdfff && value !in 0xf700..0xf8ff
  private val named = mapOf(
    KeyEvent.KEYCODE_GRAVE to 1, KeyEvent.KEYCODE_BACKSLASH to 2,
    KeyEvent.KEYCODE_LEFT_BRACKET to 3, KeyEvent.KEYCODE_RIGHT_BRACKET to 4,
    KeyEvent.KEYCODE_COMMA to 5, KeyEvent.KEYCODE_EQUALS to 16,
    KeyEvent.KEYCODE_MINUS to 46, KeyEvent.KEYCODE_PERIOD to 47, KeyEvent.KEYCODE_APOSTROPHE to 48,
    KeyEvent.KEYCODE_SEMICOLON to 49, KeyEvent.KEYCODE_SLASH to 50,
    KeyEvent.KEYCODE_ALT_LEFT to 51, KeyEvent.KEYCODE_ALT_RIGHT to 52, KeyEvent.KEYCODE_DEL to 53,
    KeyEvent.KEYCODE_CAPS_LOCK to 54, KeyEvent.KEYCODE_MENU to 55,
    KeyEvent.KEYCODE_CTRL_LEFT to 56, KeyEvent.KEYCODE_CTRL_RIGHT to 57, KeyEvent.KEYCODE_ENTER to 58,
    KeyEvent.KEYCODE_META_LEFT to 59, KeyEvent.KEYCODE_META_RIGHT to 60,
    KeyEvent.KEYCODE_SHIFT_LEFT to 61, KeyEvent.KEYCODE_SHIFT_RIGHT to 62,
    KeyEvent.KEYCODE_SPACE to 63, KeyEvent.KEYCODE_TAB to 64, KeyEvent.KEYCODE_FORWARD_DEL to 68,
    KeyEvent.KEYCODE_MOVE_END to 69, KeyEvent.KEYCODE_MOVE_HOME to 71, KeyEvent.KEYCODE_INSERT to 72,
    KeyEvent.KEYCODE_PAGE_DOWN to 73, KeyEvent.KEYCODE_PAGE_UP to 74,
    KeyEvent.KEYCODE_DPAD_DOWN to 75, KeyEvent.KEYCODE_DPAD_LEFT to 76,
    KeyEvent.KEYCODE_DPAD_RIGHT to 77, KeyEvent.KEYCODE_DPAD_UP to 78, KeyEvent.KEYCODE_NUM_LOCK to 79,
    KeyEvent.KEYCODE_NUMPAD_ADD to 90, KeyEvent.KEYCODE_NUMPAD_COMMA to 94,
    KeyEvent.KEYCODE_NUMPAD_DOT to 95, KeyEvent.KEYCODE_NUMPAD_DIVIDE to 96,
    KeyEvent.KEYCODE_NUMPAD_ENTER to 97, KeyEvent.KEYCODE_NUMPAD_EQUALS to 98,
    KeyEvent.KEYCODE_NUMPAD_MULTIPLY to 104, KeyEvent.KEYCODE_NUMPAD_LEFT_PAREN to 105,
    KeyEvent.KEYCODE_NUMPAD_RIGHT_PAREN to 106, KeyEvent.KEYCODE_NUMPAD_SUBTRACT to 107,
    KeyEvent.KEYCODE_ESCAPE to 120, KeyEvent.KEYCODE_SYSRQ to 148,
    KeyEvent.KEYCODE_SCROLL_LOCK to 149, KeyEvent.KEYCODE_BREAK to 150,
    KeyEvent.KEYCODE_FORWARD to 153, KeyEvent.KEYCODE_ENVELOPE to 161,
    KeyEvent.KEYCODE_MEDIA_PLAY_PAUSE to 162, KeyEvent.KEYCODE_MEDIA_STOP to 164,
    KeyEvent.KEYCODE_MEDIA_NEXT to 165, KeyEvent.KEYCODE_MEDIA_PREVIOUS to 166,
    KeyEvent.KEYCODE_POWER to 167, KeyEvent.KEYCODE_SLEEP to 168,
    KeyEvent.KEYCODE_VOLUME_DOWN to 169, KeyEvent.KEYCODE_VOLUME_MUTE to 170,
    KeyEvent.KEYCODE_VOLUME_UP to 171, KeyEvent.KEYCODE_WAKEUP to 172,
    KeyEvent.KEYCODE_COPY to 173, KeyEvent.KEYCODE_CUT to 174, KeyEvent.KEYCODE_PASTE to 175,
  )
}
