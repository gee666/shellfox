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
  private fun commits() = events.filter { it.optString("type") == "commit" }.map { it.getString("text") }

  @Test fun compositionDoesNotCommitUntilImeCommitAndFinishDoesNotDuplicate() {
    val ime = connection()
    ime.setComposingText("に", 1)
    ime.setComposingText("日本", 1)
    assertTrue(commits().isEmpty())
    ime.commitText("日本", 1)
    ime.finishComposingText()
    assertEquals(listOf("日本"), commits())
    assertEquals("", ime.editable.toString())
  }
  @Test fun partialUtf16CompositionNeverTurnsIntoReplacementInput() {
    val ime = connection()
    ime.setComposingText("\uD83D", 1)
    assertFalse(ime.finishComposingText())
    assertFalse(ime.commitText("\uD83D", 1))
    assertTrue(commits().isEmpty())
    ime.setComposingText("😀", 1)
    assertTrue(ime.commitText("😀", 1))
    assertEquals(listOf("😀"), commits())
  }
  @Test fun finishCommitsMarkedTextOnceAndCancelDoesNotCommit() {
    val ime = connection()
    ime.setComposingText("😀", 1)
    ime.finishComposingText()
    ime.finishComposingText()
    ime.setComposingText("discard", 1)
    ime.cancelComposition()
    assertEquals(listOf("😀"), commits())
  }
  @Test fun deletingPreeditKeepsSurrogatesWholeAndDoesNotBackspaceTheTerminal() {
    val ime = connection()
    ime.setComposingText("x😀", 1)
    ime.deleteSurroundingText(1, 0)
    assertEquals("x", ime.editable.toString())
    assertTrue(events.none { it.optString("type") == "key" })
    ime.commitText("x", 1)
    assertEquals(listOf("x"), commits())
  }
  @Test fun surroundingDeletionPreservesSelectedPreeditAndRejectsSplitSurrogateSelections() {
    val ime = connection()
    ime.setComposingText("ab😀cd", 1)
    assertTrue(ime.setSelection(2, 4))
    ime.deleteSurroundingText(0, 0)
    assertEquals("ab😀cd", ime.editable.toString())
    ime.deleteSurroundingText(1, 1)
    assertEquals("a😀d", ime.editable.toString())
    assertFalse(ime.setSelection(2, 2))
    assertFalse(ime.setComposingRegion(2, 3))
    assertTrue(commits().isEmpty())
  }
  @Test fun imeNewlineTabUseGhosttyKeysAndFunctionKeysAreNotDropped() {
    input.text("commit", "a\r\nb\tc")
    assertEquals(listOf("a", "b", "c"), commits())
    assertEquals(listOf("enter", "enter", "tab", "tab"),
      events.filter { it.optString("type") == "key" }.map { it.getJSONObject("key").getString("kind") })
    events.clear()
    assertTrue(input.hardware(KeyEvent(KeyEvent.ACTION_DOWN, KeyEvent.KEYCODE_F5)))
    assertEquals("function", events.single().getJSONObject("key").getString("kind"))
    assertEquals(5, events.single().getJSONObject("key").getInt("value"))
  }
  @Test fun virtualKeyboardDeletionEncodesKeysNotRawEscapes() {
    val ime = connection()
    ime.deleteSurroundingText(2, 1)
    assertEquals(listOf("backspace", "backspace", "backspace", "backspace", "delete", "delete"),
      events.map { it.getJSONObject("key").getString("kind") })
    assertEquals(listOf("press", "release", "press", "release", "press", "release"),
      events.map { it.getString("action") })
  }
  @Test fun stickyControlUsesPhysicalCharacterAndPreeditStaysSeparate() {
    input.stickyModifiers = 2
    input.text("preedit", "c")
    input.text("commit", "c")
    assertEquals("preedit", events[0].getString("type"))
    assertEquals("character", events[1].getJSONObject("key").getString("kind"))
    assertEquals("c", events[1].getJSONObject("key").getString("value"))
    assertEquals(2, events[1].getInt("modifiers"))
  }
  @Test fun hardwarePressRepeatReleaseStaySeparateAndFocusLossReleasesHeldKeys() {
    assertTrue(input.hardware(KeyEvent(0, 0, KeyEvent.ACTION_DOWN, KeyEvent.KEYCODE_DPAD_UP, 0)))
    assertTrue(input.hardware(KeyEvent(0, 1, KeyEvent.ACTION_DOWN, KeyEvent.KEYCODE_DPAD_UP, 1)))
    assertTrue(input.hardware(KeyEvent(0, 2, KeyEvent.ACTION_UP, KeyEvent.KEYCODE_DPAD_UP, 0)))
    assertEquals(listOf("press", "repeat", "release"), events.map { it.getString("action") })
    events.clear()
    input.hardware(KeyEvent(KeyEvent.ACTION_DOWN, KeyEvent.KEYCODE_ENTER))
    input.reset()
    assertEquals("release", events[1].getString("action"))
    assertFalse(input.hardware(KeyEvent(KeyEvent.ACTION_DOWN, KeyEvent.KEYCODE_BACK)))
    events.clear()
    input.hardware(KeyEvent(0, 0, KeyEvent.ACTION_DOWN, KeyEvent.KEYCODE_DPAD_RIGHT, 0,
      KeyEvent.META_CTRL_ON or KeyEvent.META_ALT_ON or KeyEvent.META_ALT_RIGHT_ON))
    assertEquals(6, events.single().getInt("modifiers"))
    assertEquals(0, events.single().getInt("consumedModifiers"))
  }
  @Test fun readonlyImeRejectsEditsAndUtf16SelectionsAreBounded() {
    val ime = TerminalInputConnection(View(RuntimeEnvironment.getApplication()), input, {}, { false })
    assertFalse(ime.commitText("x", 1))
    assertFalse(ime.setComposingText("x", 1))
    assertFalse(ime.deleteSurroundingText(1, 0))
    assertFalse(ime.setSelection(-1, 3))
    assertTrue(commits().isEmpty())
  }
  @Test fun retiredImeCannotCommitAfterTheNativeBindingChanges() {
    var current = true
    val ime = TerminalInputConnection(View(RuntimeEnvironment.getApplication()), input, {}, { current })
    ime.setComposingText("old", 1)
    current = false
    assertFalse(ime.commitText("old", 1))
    ime.finishComposingText()
    assertTrue(commits().isEmpty())
    events.clear()
    input.hardware(KeyEvent(KeyEvent.ACTION_DOWN, KeyEvent.KEYCODE_ENTER))
    input.reset(releaseHeld = false)
    assertTrue(events.none { it.optString("action") == "release" })
  }
  @Test fun untrustedOsc8LinksAllowOnlyExplicitHttpSchemes() {
    assertNotNull(TerminalTouch.safeLink("https://example.invalid/a"))
    for (link in listOf("javascript:alert(1)", "file:///tmp/data", "intent://a", "https://user:secret@example.invalid", "https://a\n.invalid"))
      assertNull(TerminalTouch.safeLink(link))
  }
}
