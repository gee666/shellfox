package com.shellfox.mobile.rust

import android.view.KeyCharacterMap
import android.view.KeyEvent
import org.json.JSONObject

/** Keys, pastes and keyboard-editor text as Rust input JSON. Ghostty encodes. */
internal class TerminalInput(private val send: (JSONObject) -> Unit) {
  /** Key-bar modifiers (shift 1, ctrl 2, alt 4, super 8) held for the next key. */
  var stickyModifiers = 0
  private var accent = 0
  private val pressed = mutableMapOf<Int, JSONObject>()
  private val typedKeys = mutableSetOf<Int>()

  fun editor(committed: String) = send(JSONObject().put("type", "editor").put("committed", committed))
  fun editorReset() = send(JSONObject().put("type", "editorReset"))
  /** Pastes go in pieces that each fit one input write. */
  fun paste(text: String) {
    var start = 0
    while (start < text.length) {
      var end = minOf(text.length, start + PASTE_CHUNK)
      if (end < text.length && Character.isHighSurrogate(text[end - 1])) end--
      send(JSONObject().put("type", "paste").put("text", text.substring(start, end)))
      start = end
    }
  }
  /** Input built elsewhere (the key bar), sent in order with the keyboard's. */
  fun forward(json: JSONObject) = send(json)

  /** Press and release, as from the key bar or an IME editor action. */
  fun tap(physical: Int, modifiers: Int = stickyModifiers, text: String = "", unshifted: Int? = null) {
    val press = key(physical, "press", modifiers, text, unshifted)
    send(press); send(release(press))
  }

  /** A soft-keyboard character typed while a key-bar modifier is held. */
  fun chord(codepoint: Int) {
    when (codepoint) {
      10, 13 -> return tap(ENTER)
      9 -> return tap(TAB)
    }
    val base = Character.toLowerCase(codepoint)
    tap(usPosition(base), stickyModifiers, String(Character.toChars(codepoint)), base.takeIf(::printable))
  }

  fun reset(releaseHeld: Boolean = true) {
    if (releaseHeld) pressed.values.forEach { send(release(it)) }
    pressed.clear()
    typedKeys.clear()
    accent = 0
  }

  /** A hardware (or IME-synthesized) key. False lets Android handle it. */
  fun hardware(event: KeyEvent, preedit: (String) -> Unit): Boolean {
    if (event.action != KeyEvent.ACTION_DOWN && event.action != KeyEvent.ACTION_UP) return false
    val physical = physical(event.keyCode) ?: return typedOnly(event)
    if (event.action == KeyEvent.ACTION_UP) {
      // Only keys whose press reached the terminal are released there.
      pressed.remove(event.keyCode)?.let { send(release(it)) }
      return true
    }
    // Layout text without Ctrl, which Ghostty applies itself. Right Alt is AltGr.
    val meta = event.metaState
    val altGr = meta and KeyEvent.META_ALT_RIGHT_ON != 0
    val layout = meta and KeyEvent.META_CTRL_MASK.inv() and (if (altGr) -1 else KeyEvent.META_ALT_MASK.inv())
    var unicode = event.getUnicodeChar(layout)
    if (unicode and KeyCharacterMap.COMBINING_ACCENT != 0) {
      accent = unicode and KeyCharacterMap.COMBINING_ACCENT_MASK
      preedit(String(Character.toChars(accent)))
      return true
    }
    if (accent != 0) {
      val combined = KeyCharacterMap.getDeadChar(accent, unicode)
      // No combination: the accent stands alone, then the key types as usual.
      if (combined == 0 && printable(accent)) tap(0, 0, String(Character.toChars(accent)))
      unicode = if (combined != 0) combined else unicode
      accent = 0
      preedit("")
    }
    var modifiers = stickyModifiers or modifierBits(meta)
    if (altGr && printable(unicode)) modifiers = modifiers and (CTRL or ALT).inv()
    val action = if (event.repeatCount > 0) "repeat" else "press"
    val unshifted = event.getUnicodeChar(0).takeIf(::printable)
    val text = if (printable(unicode)) String(Character.toChars(unicode)) else ""
    val consumed = if (text.isNotEmpty() && event.isShiftPressed && unicode != unshifted) SHIFT else 0
    val json = key(physical, action, modifiers, text, unshifted, consumed)
    pressed[event.keyCode] = json
    send(json)
    return true
  }

  /** A key with no Ghostty identity that still types a character (KEYCODE_AT, _POUND...). */
  private fun typedOnly(event: KeyEvent): Boolean {
    if (event.action == KeyEvent.ACTION_UP) return typedKeys.remove(event.keyCode)
    val unicode = event.getUnicodeChar(event.metaState and KeyEvent.META_CTRL_MASK.inv())
    if (!printable(unicode)) return false
    typedKeys.add(event.keyCode)
    tap(0, stickyModifiers, String(Character.toChars(unicode)))
    return true
  }

  companion object {
    const val SHIFT = 1
    const val CTRL = 2
    const val ALT = 4
    const val SUPER = 8
    const val ENTER = 58
    const val BACKSPACE = 53
    const val TAB = 64
    // UTF-16 units; at most 3 UTF-8 bytes each, well under the 64 KiB write.
    private const val PASTE_CHUNK = 16 * 1024

    fun key(physical: Int, action: String, modifiers: Int, text: String = "", unshifted: Int? = null,
      consumed: Int = 0): JSONObject =
      JSONObject().put("type", "key").put("event", JSONObject().put("physical", physical)
        .put("action", action).put("modifiers", modifiers).put("consumedModifiers", consumed)
        .put("text", text).put("unshiftedCodepoint", unshifted ?: JSONObject.NULL).put("composing", false))

    fun release(press: JSONObject): JSONObject = JSONObject(press.toString()).also {
      it.getJSONObject("event").put("action", "release").put("text", "")
    }

    fun validUnicode(text: String): Boolean {
      var index = 0
      while (index < text.length) {
        val char = text[index++]
        if (char.isHighSurrogate()) {
          if (index >= text.length || !text[index++].isLowSurrogate()) return false
        } else if (char.isLowSurrogate()) return false
      }
      return true
    }

    private fun printable(value: Int): Boolean = Character.isValidCodePoint(value) && value >= 32 &&
      value != 127 && value !in 0xd800..0xdfff && value !in 0xf700..0xf8ff

    private fun modifierBits(meta: Int): Int =
      (if (meta and KeyEvent.META_SHIFT_ON != 0) SHIFT else 0) or
        (if (meta and KeyEvent.META_CTRL_ON != 0) CTRL else 0) or
        (if (meta and KeyEvent.META_ALT_ON != 0) ALT else 0) or
        (if (meta and KeyEvent.META_META_ON != 0) SUPER else 0) or
        (if (meta and KeyEvent.META_CAPS_LOCK_ON != 0) 16 else 0) or
        (if (meta and KeyEvent.META_NUM_LOCK_ON != 0) 32 else 0)

    /** The US key that types this ASCII character unshifted, else Unidentified. */
    fun usPosition(codepoint: Int): Int = when (codepoint) {
      in 'a'.code..'z'.code -> 20 + codepoint - 'a'.code
      in '0'.code..'9'.code -> 6 + codepoint - '0'.code
      else -> mapOf('`' to 1, '\\' to 2, '[' to 3, ']' to 4, ',' to 5, '=' to 16, '-' to 46, '.' to 47,
        '\'' to 48, ';' to 49, '/' to 50, ' ' to 63).entries.firstOrNull { it.key.code == codepoint }?.value ?: 0
    }

    /** Android key code to the public GhosttyKey number. System keys stay Android's. */
    fun physical(code: Int): Int? = when (code) {
      in KeyEvent.KEYCODE_A..KeyEvent.KEYCODE_Z -> 20 + code - KeyEvent.KEYCODE_A
      in KeyEvent.KEYCODE_0..KeyEvent.KEYCODE_9 -> 6 + code - KeyEvent.KEYCODE_0
      in KeyEvent.KEYCODE_F1..KeyEvent.KEYCODE_F12 -> 121 + code - KeyEvent.KEYCODE_F1
      in KeyEvent.KEYCODE_NUMPAD_0..KeyEvent.KEYCODE_NUMPAD_9 -> 80 + code - KeyEvent.KEYCODE_NUMPAD_0
      else -> named[code]
    }
    private val named = mapOf(
      KeyEvent.KEYCODE_GRAVE to 1, KeyEvent.KEYCODE_BACKSLASH to 2,
      KeyEvent.KEYCODE_LEFT_BRACKET to 3, KeyEvent.KEYCODE_RIGHT_BRACKET to 4,
      KeyEvent.KEYCODE_COMMA to 5, KeyEvent.KEYCODE_EQUALS to 16,
      KeyEvent.KEYCODE_MINUS to 46, KeyEvent.KEYCODE_PERIOD to 47, KeyEvent.KEYCODE_APOSTROPHE to 48,
      KeyEvent.KEYCODE_SEMICOLON to 49, KeyEvent.KEYCODE_SLASH to 50,
      KeyEvent.KEYCODE_ALT_LEFT to 51, KeyEvent.KEYCODE_ALT_RIGHT to 52, KeyEvent.KEYCODE_DEL to BACKSPACE,
      KeyEvent.KEYCODE_CAPS_LOCK to 54, KeyEvent.KEYCODE_MENU to 55,
      KeyEvent.KEYCODE_CTRL_LEFT to 56, KeyEvent.KEYCODE_CTRL_RIGHT to 57, KeyEvent.KEYCODE_ENTER to ENTER,
      KeyEvent.KEYCODE_META_LEFT to 59, KeyEvent.KEYCODE_META_RIGHT to 60,
      KeyEvent.KEYCODE_SHIFT_LEFT to 61, KeyEvent.KEYCODE_SHIFT_RIGHT to 62,
      KeyEvent.KEYCODE_SPACE to 63, KeyEvent.KEYCODE_TAB to TAB, KeyEvent.KEYCODE_FORWARD_DEL to 68,
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
    )
  }
}
