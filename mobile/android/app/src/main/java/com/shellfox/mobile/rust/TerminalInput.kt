package com.shellfox.mobile.rust

import android.view.KeyCharacterMap
import android.view.KeyEvent
import org.json.JSONObject

internal class TerminalInput(private val send: (JSONObject) -> Unit) {
  var stickyModifiers = 0
  private var accent = 0
  private val pressed = mutableMapOf<Int, JSONObject>()

  fun text(type: String, text: String) {
    if (type == "preedit" && !validUnicode(text)) return
    require(validUnicode(text))
    if (type == "commit" && text.any { it == '\r' || it == '\n' || it == '\t' }) {
      var start = 0
      for (match in Regex("\\r\\n|[\\r\\n\\t]").findAll(text)) {
        if (match.range.first > start) text("commit", text.substring(start, match.range.first))
        val kind = if (match.value == "\t") "tab" else "enter"
        key(kind); key(kind, "release")
        start = match.range.last + 1
      }
      if (start < text.length) text("commit", text.substring(start))
      return
    }
    if (type == "commit" && stickyModifiers != 0) {
      val key = if (text.length == 1 && text[0].code in 32..126)
        JSONObject().put("kind", "character").put("value", text.lowercase(java.util.Locale.ROOT))
        else JSONObject().put("kind", "unidentified")
      val event = JSONObject().put("type", "key").put("key", key)
        .put("action", "press").put("text", text).put("modifiers", stickyModifiers)
      send(event)
      send(JSONObject(event.toString()).apply { put("action", "release"); remove("text") })
    } else send(JSONObject().put("type", type).put("text", text))
  }

  fun key(kind: String, action: String = "press") {
    send(JSONObject().put("type", "key").put("key", JSONObject().put("kind", kind))
      .put("action", action).put("modifiers", stickyModifiers))
  }

  fun reset(releaseHeld: Boolean = true) {
    if (releaseHeld) pressed.values.forEach { event ->
      send(JSONObject(event.toString()).apply { put("action", "release"); remove("text") })
    }
    accent = 0
    pressed.clear()
    text("preedit", "")
  }

  fun hardware(event: KeyEvent): Boolean {
    if (event.action == KeyEvent.ACTION_MULTIPLE) {
      event.characters?.let { if (!validUnicode(it)) return false; text("commit", it); return true }
      return false
    }
    if (event.keyCode in modifierKeys || event.keyCode == KeyEvent.KEYCODE_BACK) return false
    if (event.action != KeyEvent.ACTION_DOWN && event.action != KeyEvent.ACTION_UP) return false
    val kind = specialKeys[event.keyCode]
    val altGr = event.metaState and KeyEvent.META_ALT_RIGHT_ON != 0
    val unicode = event.getUnicodeChar(if (altGr) event.metaState and KeyEvent.META_CTRL_MASK.inv() else event.metaState)
    if (kind == null && unicode == 0 && event.keyCode !in KeyEvent.KEYCODE_F1..KeyEvent.KEYCODE_F12 && event.action == KeyEvent.ACTION_DOWN) return false
    if (unicode and KeyCharacterMap.COMBINING_ACCENT != 0 && event.action == KeyEvent.ACTION_DOWN) {
      accent = unicode and KeyCharacterMap.COMBINING_ACCENT_MASK
      text("preedit", String(Character.toChars(accent)))
      return true
    }
    if (event.action == KeyEvent.ACTION_UP && pressed.remove(event.keyCode) == null) return true
    var logical = unicode
    if (accent != 0 && event.action == KeyEvent.ACTION_DOWN) {
      val combined = KeyCharacterMap.getDeadChar(accent, unicode)
      if (combined == 0) text("commit", String(Character.toChars(accent)))
      logical = if (combined == 0) unicode else combined
      accent = 0
      text("preedit", "")
    }
    val unshifted = event.getUnicodeChar(0)
    val key = JSONObject()
    when {
      kind != null -> key.put("kind", kind)
      event.keyCode in KeyEvent.KEYCODE_F1..KeyEvent.KEYCODE_F12 ->
        key.put("kind", "function").put("value", event.keyCode - KeyEvent.KEYCODE_F1 + 1)
      unshifted in 32..126 -> key.put("kind", "character").put("value", unshifted.toChar().toString())
      else -> key.put("kind", "unidentified")
    }
    val modifiers = stickyModifiers or (if (event.isShiftPressed) 1 else 0) or
      (if (event.isCtrlPressed) 2 else 0) or (if (event.isAltPressed) 4 else 0) or (if (event.isMetaPressed) 8 else 0)
    val action = if (event.action == KeyEvent.ACTION_UP) "release" else if (event.repeatCount > 0) "repeat" else "press"
    val json = JSONObject().put("type", "key").put("key", key).put("action", action)
      .put("modifiers", modifiers).put("consumedModifiers", if (altGr && kind == null && logical >= 32) modifiers and 6 else 0)
      .put("unshiftedCodepoint", unshifted.coerceAtLeast(0))
    if (event.action == KeyEvent.ACTION_DOWN && kind == null && logical >= 32 && logical !in 0xd800..0xdfff && Character.isValidCodePoint(logical))
      json.put("text", String(Character.toChars(logical)))
    if (event.action == KeyEvent.ACTION_DOWN) pressed[event.keyCode] = json
    send(json)
    return true
  }

  companion object {
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
    private val modifierKeys = setOf(KeyEvent.KEYCODE_SHIFT_LEFT, KeyEvent.KEYCODE_SHIFT_RIGHT,
      KeyEvent.KEYCODE_CTRL_LEFT, KeyEvent.KEYCODE_CTRL_RIGHT, KeyEvent.KEYCODE_ALT_LEFT,
      KeyEvent.KEYCODE_ALT_RIGHT, KeyEvent.KEYCODE_META_LEFT, KeyEvent.KEYCODE_META_RIGHT)
    private val specialKeys = mapOf(
      KeyEvent.KEYCODE_ENTER to "enter", KeyEvent.KEYCODE_NUMPAD_ENTER to "enter",
      KeyEvent.KEYCODE_TAB to "tab", KeyEvent.KEYCODE_DEL to "backspace", KeyEvent.KEYCODE_ESCAPE to "escape",
      KeyEvent.KEYCODE_DPAD_UP to "arrowUp", KeyEvent.KEYCODE_DPAD_DOWN to "arrowDown",
      KeyEvent.KEYCODE_DPAD_LEFT to "arrowLeft", KeyEvent.KEYCODE_DPAD_RIGHT to "arrowRight",
      KeyEvent.KEYCODE_MOVE_HOME to "home", KeyEvent.KEYCODE_MOVE_END to "end",
      KeyEvent.KEYCODE_PAGE_UP to "pageUp", KeyEvent.KEYCODE_PAGE_DOWN to "pageDown",
      KeyEvent.KEYCODE_INSERT to "insert", KeyEvent.KEYCODE_FORWARD_DEL to "delete",
    )
  }
}
