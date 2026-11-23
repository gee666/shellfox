package com.shellfox.mobile.rust

import android.os.Handler
import android.os.HandlerThread
import android.view.KeyEvent
import android.view.View
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith
import java.util.concurrent.FutureTask
import java.util.concurrent.TimeUnit

/**
 * Keyboard call sequences as Gboard, Samsung Keyboard and voice typing make
 * them, on the device's real InputConnection, JNI and Ghostty encoder. The
 * bytes land in a line editor that behaves like a shell prompt.
 */
@RunWith(AndroidJUnit4::class)
class SoftKeyboardEditsTest {
  private val instrumentation = InstrumentationRegistry.getInstrumentation()
  private lateinit var thread: HandlerThread
  private lateinit var owner: Handler
  private var terminal = 0L
  private val line = StringBuilder()
  private val submitted = mutableListOf<String>()
  private lateinit var input: TerminalInput
  private lateinit var ime: TerminalInputConnection

  private fun <T> native(action: () -> T): T {
    val task = FutureTask(action)
    owner.post(task)
    return task.get(5, TimeUnit.SECONDS)
  }
  private fun shell(bytes: ByteArray) {
    for (c in String(bytes, Charsets.UTF_8).codePoints().toArray()) when (c) {
      0x7f -> if (line.isNotEmpty()) line.setLength(line.offsetByCodePoints(line.length, -1))
      '\r'.code -> { submitted.add(line.toString()); line.clear() }
      else -> { require(c >= 32) { "unexpected control $c" }; line.appendCodePoint(c) }
    }
  }
  private fun call(action: TerminalInputConnection.() -> Unit) = instrumentation.runOnMainSync { ime.action() }

  @Before fun create() {
    thread = HandlerThread("SoftKeyboardOwner").apply { start() }
    owner = Handler(thread.looper)
    terminal = native { RustBindings.create(80, 24) }
    input = TerminalInput { json ->
      val encoded = native { EncodedInput.decode(RustBindings.inputEx(terminal, json.toString().toByteArray())) }
      encoded?.let { shell(it.bytes) }
    }
    instrumentation.runOnMainSync {
      ime = TerminalInputConnection(View(instrumentation.targetContext), input, {}, { true })
    }
  }
  @After fun close() {
    if (terminal != 0L) native { RustBindings.close(terminal) }
    thread.quitSafely()
    thread.join(5000)
  }

  @Test fun composingTextStaysLocalAndALateFinishDoesNotRetype() {
    for (prefix in listOf("g", "gi", "git")) call { setComposingText(prefix, 1) }
    assertEquals("", line.toString())
    call { commitText("git ", 1) }
    call { finishComposingText() }
    assertEquals("git ", line.toString())
  }
  @Test fun gboardAutocorrectReplacesThePreviousWordAfterSpace() {
    call { setComposingText("teh", 1) }
    call { commitText("the ", 1) }
    call { commitText("sl ", 1) }
    assertEquals("the sl ", line.toString())
    call { setComposingRegion(4, 6) }
    call { setComposingText("ls", 1) }
    call { finishComposingText() }
    assertEquals("the ls ", line.toString())
  }
  @Test fun tappingASuggestionReplacesAWordMidLine() {
    call { commitText("git stauts --short", 1) }
    call { setComposingRegion(4, 10) }
    call { commitText("status", 1) }
    assertEquals("git status --short", line.toString())
    call { performEditorAction(0) }
    assertEquals(listOf("git status --short"), submitted)
  }
  @Test fun samsungReplacesWithDeleteSurroundingThenCommit() {
    call { commitText("cd projetcs", 1) }
    call { deleteSurroundingText(8, 0) }
    call { commitText("projects", 1) }
    assertEquals("cd projects", line.toString())
  }
  @Test fun swipeTypingAWordThenReplacingItFromTheStrip() {
    call { setComposingText("hello", 1) }
    call { finishComposingText() }
    call { commitText(" ", 1) }
    call { setComposingText("word", 1) }
    call { finishComposingText() }
    assertEquals("hello word", line.toString())
    call { setComposingRegion(6, 10) }
    call { commitText("world", 1) }
    assertEquals("hello world", line.toString())
  }
  @Test fun voiceInputCommitsOnlyTheFinalPhrase() {
    for (partial in listOf("echo", "echo hell", "echo hello", "echo hello world")) {
      call { setComposingText(partial, 1) }
      assertEquals("", line.toString())
    }
    call { commitText("echo hello world", 1) }
    assertEquals("echo hello world", line.toString())
  }
  @Test fun emojiAndBackspaceThroughCommittedAndPreeditText() {
    call { commitText("ok 👍🏽", 1) }
    assertEquals("ok 👍🏽", line.toString())
    call { deleteSurroundingTextInCodePoints(2, 0) }
    assertEquals("ok ", line.toString())
    call { setComposingText("wo", 1) }
    call { setComposingText("w", 1) }
    call { setComposingText("", 1) }
    assertEquals("ok ", line.toString())
    // Gboard's Backspace key while the editor has text, then with it empty.
    for (i in 0 until 4) call {
      sendKeyEvent(KeyEvent(KeyEvent.ACTION_DOWN, KeyEvent.KEYCODE_DEL))
      sendKeyEvent(KeyEvent(KeyEvent.ACTION_UP, KeyEvent.KEYCODE_DEL))
    }
    assertEquals("", line.toString())
  }
  @Test fun enterInTheMiddleOfCompositionCommitsThenSubmits() {
    call { commitText("echo ", 1) }
    call { setComposingText("hel", 1) }
    call { performEditorAction(0) }
    assertEquals(listOf("echo hel"), submitted)
    call { setComposingText("ls", 1) }
    call { commitText("ls\n", 1) }
    assertEquals(listOf("echo hel", "ls"), submitted)
    call { commitText("pwd", 1) }
    assertEquals("pwd", line.toString())
  }
  @Test fun stickyCtrlTurnsTheNextTypedLetterIntoAChord() {
    var bytes = byteArrayOf()
    input = TerminalInput { json ->
      native { EncodedInput.decode(RustBindings.inputEx(terminal, json.toString().toByteArray())) }
        ?.let { bytes += it.bytes }
    }
    instrumentation.runOnMainSync { ime = TerminalInputConnection(View(instrumentation.targetContext), input, {}, { true }) }
    input.stickyModifiers = TerminalInput.CTRL
    call { setComposingText("c", 1) }
    input.stickyModifiers = 0
    call { commitText("ls", 1) }
    assertEquals("\u0003ls", String(bytes, Charsets.UTF_8))
  }
}
