package com.shellfox.mobile.rust

import android.view.KeyEvent
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(manifest = Config.NONE, sdk = [24], application = android.app.Application::class)
class ManagerKeyInputTest {
  @Test fun physicalComesFromAndroidCodeNeverFromLayoutText() {
    val source = KeyEvent(KeyEvent.ACTION_DOWN, KeyEvent.KEYCODE_A)
    val key = ManagerKeyInput.event(source, '中'.code, 'a'.code, 0, "press")!!.getJSONObject("event")
    assertEquals(20, key.getInt("physical"))
    assertEquals("中", key.getString("text"))
    assertEquals('a'.code, key.getInt("unshiftedCodepoint"))
    assertNull(ManagerKeyInput.physical(KeyEvent.KEYCODE_UNKNOWN))
  }
  @Test fun sideAndLockMetadataStayInClosedMaskWithoutInventedConsumedBits() {
    val meta = KeyEvent.META_SHIFT_ON or KeyEvent.META_CTRL_ON or KeyEvent.META_ALT_ON or KeyEvent.META_META_ON or
      KeyEvent.META_SHIFT_RIGHT_ON or KeyEvent.META_CTRL_RIGHT_ON or KeyEvent.META_ALT_RIGHT_ON or KeyEvent.META_META_RIGHT_ON or
      KeyEvent.META_CAPS_LOCK_ON or KeyEvent.META_NUM_LOCK_ON
    val source = KeyEvent(0, 1, KeyEvent.ACTION_DOWN, KeyEvent.KEYCODE_DPAD_RIGHT, 0, meta)
    val key = ManagerKeyInput.event(source, 0, 0, 0, "press")!!.getJSONObject("event")
    assertEquals(1023, key.getInt("modifiers"))
    assertEquals(0, key.getInt("consumedModifiers"))
    assertEquals(77, key.getInt("physical"))
  }
  @Test fun nativeReleaseClearsTextAndPreservesPhysicalAndSideIdentity() {
    val pressed = ManagerKeyInput.logical(20, modifiers = 129, text = "A", unshifted = 'a'.code)
    val key = ManagerKeyInput.release(pressed).getJSONObject("event")
    assertEquals("release", key.getString("action")); assertEquals("", key.getString("text"))
    assertEquals(20, key.getInt("physical")); assertEquals(129, key.getInt("modifiers"))
    assertEquals("press", pressed.getJSONObject("event").getString("action"))
  }
  @Test fun managerImeIsLogicalAndDoesNotTurnPreeditIntoHardware() {
    val events = mutableListOf<JSONObject>()
    val input = TerminalInput { events.add(it) }.apply { managerV2 = true; stickyModifiers = 2 }
    input.text("preedit", "e\u0301")
    input.text("commit", "e\u0301")
    assertEquals("preedit", events[0].getString("type"))
    assertEquals(0, events[1].getJSONObject("event").getInt("physical"))
    assertEquals("e\u0301", events[1].getJSONObject("event").getString("text"))
    assertEquals("release", events[2].getJSONObject("event").getString("action"))
  }
}
