package com.shellfox.mobile.rust

import android.view.KeyEvent
import android.view.View
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(manifest = Config.NONE, sdk = [24], application = android.app.Application::class)
class TerminalInputTest {
  private val events = mutableListOf<JSONObject>()
  private val input = TerminalInput { events.add(it) }
  private fun connection() = TerminalInputConnection(View(RuntimeEnvironment.getApplication()), input, {}, { true })
  private fun editors() = events.filter { it.optString("type") == "editor" }.map { it.getString("committed") }
  private fun keys() = events.filter { it.optString("type") == "key" }.map { it.getJSONObject("event") }

  @Test fun theEditorReportsCommittedTextAndKeepsCompositionLocal() {
    val ime = connection()
    ime.setComposingText("に", 1)
    ime.setComposingText("日本", 1)
    assertTrue(editors().isEmpty())
    ime.commitText("日本", 1)
    ime.finishComposingText()
    assertEquals(listOf("日本"), editors())
    // The editor keeps the text so the keyboard can correct it.
    assertEquals("日本", ime.editable.toString())
  }
  @Test fun replacingAWordReportsTheWholeEditedText() {
    val ime = connection()
    ime.commitText("git stauts", 1)
    ime.setComposingRegion(4, 10)
    ime.commitText("status", 1)
    assertEquals(listOf("git stauts", "git ", "git status"), editors())
  }
  @Test fun partialUtf16TextIsRejected() {
    val ime = connection()
    assertFalse(ime.commitText("\uD83D", 1))
    assertTrue(editors().isEmpty())
    assertTrue(ime.commitText("😀", 1))
    assertEquals(listOf("😀"), editors())
  }
  @Test fun enterAndKeysClearTheEditorFirst() {
    val ime = connection()
    ime.setComposingText("ls", 1)
    ime.performEditorAction(0)
    assertEquals(listOf("editor", "editorReset", "key", "key"), events.map { it.getString("type") })
    assertEquals(listOf(TerminalInput.ENTER, TerminalInput.ENTER), keys().map { it.getInt("physical") })
    assertEquals("", ime.editable.toString())
    events.clear()
    ime.commitText("a", 1)
    ime.sendKeyEvent(KeyEvent(KeyEvent.ACTION_DOWN, KeyEvent.KEYCODE_DPAD_LEFT))
    assertEquals(listOf("editor", "editorReset", "key"), events.map { it.getString("type") })
    assertEquals(76, keys().single().getInt("physical"))
  }
  @Test fun backspaceEditsTheEditorThenTheTerminal() {
    val ime = connection()
    ime.commitText("a😀", 1)
    ime.sendKeyEvent(KeyEvent(KeyEvent.ACTION_DOWN, KeyEvent.KEYCODE_DEL))
    assertEquals("a", ime.editable.toString())
    ime.deleteSurroundingText(1, 0)
    assertEquals(listOf("a😀", "a", ""), editors())
    assertTrue(keys().isEmpty())
    ime.deleteSurroundingText(2, 0)
    assertEquals(listOf("press", "release", "press", "release"), keys().map { it.getString("action") })
    assertTrue(keys().all { it.getInt("physical") == TerminalInput.BACKSPACE })
  }
  @Test fun stickyControlTurnsTheTypedLetterIntoAChord() {
    val ime = connection()
    input.stickyModifiers = TerminalInput.CTRL
    ime.setComposingText("c", 1)
    val key = keys().first()
    assertEquals(22, key.getInt("physical"))
    assertEquals("c", key.getString("text"))
    assertEquals(TerminalInput.CTRL, key.getInt("modifiers"))
    assertEquals("", ime.editable.toString())
    assertTrue(editors().isEmpty())
  }
  @Test fun hardwarePressRepeatReleaseAndFocusLossReleasesHeldKeys() {
    val preedit = { _: String -> }
    assertTrue(input.hardware(KeyEvent(0, 0, KeyEvent.ACTION_DOWN, KeyEvent.KEYCODE_DPAD_UP, 0), preedit))
    assertTrue(input.hardware(KeyEvent(0, 1, KeyEvent.ACTION_DOWN, KeyEvent.KEYCODE_DPAD_UP, 1), preedit))
    assertTrue(input.hardware(KeyEvent(0, 2, KeyEvent.ACTION_UP, KeyEvent.KEYCODE_DPAD_UP, 0), preedit))
    assertEquals(listOf("press", "repeat", "release"), keys().map { it.getString("action") })
    events.clear()
    input.hardware(KeyEvent(KeyEvent.ACTION_DOWN, KeyEvent.KEYCODE_ENTER), preedit)
    input.reset()
    assertEquals("release", keys()[1].getString("action"))
    assertFalse(input.hardware(KeyEvent(KeyEvent.ACTION_DOWN, KeyEvent.KEYCODE_BACK), preedit))
    assertFalse(input.hardware(KeyEvent(KeyEvent.ACTION_DOWN, KeyEvent.KEYCODE_VOLUME_UP), preedit))
    events.clear()
    input.hardware(KeyEvent(0, 0, KeyEvent.ACTION_DOWN, KeyEvent.KEYCODE_C, 0, KeyEvent.META_CTRL_ON or KeyEvent.META_CTRL_LEFT_ON), preedit)
    val ctrl = keys().single()
    assertEquals(22, ctrl.getInt("physical"))
    assertEquals("c", ctrl.getString("text"))
    assertEquals(TerminalInput.CTRL, ctrl.getInt("modifiers"))
    assertEquals(99, ctrl.getInt("unshiftedCodepoint"))
  }
  @Test fun readonlyImeRejectsEdits() {
    val ime = TerminalInputConnection(View(RuntimeEnvironment.getApplication()), input, {}, { false })
    assertFalse(ime.commitText("x", 1))
    assertFalse(ime.setComposingText("x", 1))
    assertFalse(ime.deleteSurroundingText(1, 0))
    assertTrue(editors().isEmpty())
  }
  @Test fun untrustedOsc8LinksAllowOnlyExplicitHttpSchemes() {
    assertNotNull(TerminalTouch.safeLink("https://example.invalid/a"))
    for (link in listOf("javascript:alert(1)", "file:///tmp/data", "intent://a", "https://user:secret@example.invalid", "https://a\n.invalid"))
      assertNull(TerminalTouch.safeLink(link))
  }
}
