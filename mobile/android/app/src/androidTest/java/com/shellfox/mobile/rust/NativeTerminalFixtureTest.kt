package com.shellfox.mobile.rust

import android.os.Handler
import android.os.HandlerThread
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import org.json.JSONObject
import org.junit.After
import org.junit.Assert.*
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith
import java.util.concurrent.FutureTask
import java.util.concurrent.TimeUnit

/** Isolated ANSI fixtures only. No tsnet startup, host/account or local PTY. */
@RunWith(AndroidJUnit4::class)
class NativeTerminalFixtureTest {
  private lateinit var thread: HandlerThread
  private lateinit var owner: Handler
  private var terminal = 0L
  private fun <T> native(action: () -> T): T {
    val task = FutureTask(action)
    owner.post(task)
    return task.get(5, TimeUnit.SECONDS)
  }
  @Before fun create() {
    thread = HandlerThread("ShellfoxFixtureOwner").apply { start() }
    owner = Handler(thread.looper)
    terminal = native { RustBindings.create(20, 5) }
  }
  @After fun close() {
    if (terminal != 0L) native { RustBindings.close(terminal) }
    thread.quitSafely()
    thread.join(5000)
  }
  @Test fun parserCreditsOnlyPumpedBytesAndSelectionCopiesNativeText() {
    assertEquals(0, RustBindings.submit(terminal, 1, "native界\r\nfixture".toByteArray()))
    assertEquals(0L, RustBindings.progress(terminal)[1])
    val frame = native { TerminalFrame.decode(RustBindings.pumpFrame(terminal)) }
    assertTrue(frame.text().contains("native界"))
    assertEquals(1L, RustBindings.progress(terminal)[1])
    val selected = native {
      RustBindings.select(terminal, 3, 0, 0, 0, 0)
      RustBindings.copy(terminal).toString(Charsets.UTF_8)
    }
    assertTrue(selected.contains("native界"))
  }
  @Test fun eitherHalfOfAWrappedPlaintextPathResolvesThroughJni() {
    val path = "./src/deep/界/terminal.rs:12:4"
    assertEquals(0, RustBindings.submit(terminal, 1, "$path\r\nnot a link".toByteArray()))
    native {
      RustBindings.pumpFrame(terminal)
      assertEquals(path, RustBindings.fileLink(terminal, 12, 0).toString(Charsets.UTF_8))
      assertEquals(path, RustBindings.fileLink(terminal, 1, 1).toString(Charsets.UTF_8))
      assertTrue(RustBindings.fileLink(terminal, 1, 2).isEmpty())
    }
  }
  @Test fun editorResetIsEmptyAndGhosttyEncodesApplicationCursorAndBracketedPaste() {
    RustBindings.submit(terminal, 1, "\u001b[?1h\u001b[?2004h".toByteArray())
    native {
      RustBindings.pumpFrame(terminal)
      assertTrue(RustBindings.input(terminal, """{"type":"editorReset"}""".toByteArray()).isEmpty())
      assertArrayEquals("\u001bOA".toByteArray(), RustBindings.input(terminal,
        """{"type":"key","event":{"physical":78,"action":"press","modifiers":0,"consumedModifiers":0,"text":"","unshiftedCodepoint":null,"composing":false}}""".toByteArray()))
      assertArrayEquals("\u001b[200~x\ny\u001b[201~".toByteArray(), RustBindings.input(terminal,
        """{"type":"paste","text":"x\ny"}""".toByteArray()))
    }
  }
  @Test fun extendedEncoderCopiesRustHintAndEmbeddedNulWithoutStringConversion() {
    native {
      val committed = requireNotNull(EncodedInput.decode(RustBindings.inputEx(terminal,
        """{"type":"editor","committed":"x"}""".toByteArray())))
      assertTrue(committed.userInitiated)
      assertArrayEquals(byteArrayOf('x'.code.toByte()), committed.bytes)
      assertTrue(RustBindings.inputEx(terminal, """{"type":"editorReset"}""".toByteArray()).isEmpty())
      val nul = requireNotNull(EncodedInput.decode(RustBindings.inputEx(terminal,
        """{"type":"key","event":{"physical":63,"action":"press","modifiers":2,"consumedModifiers":0,"text":" ","unshiftedCodepoint":32,"composing":false}}""".toByteArray())))
      assertTrue(nul.userInitiated)
      assertArrayEquals(byteArrayOf(0), nul.bytes)
    }
  }
  @Test fun nulAndPassiveHintsReachTheRealRustClientWireThroughJni() {
    val assets = InstrumentationRegistry.getInstrumentation().context.assets
    FixtureTransport.initialize(assets.open("remote-pair-success.json").use { it.readBytes() },
      assets.open("terminal-attach-owned-success.json").use { it.readBytes() })
    val client = native { ClientBindings.create("n-fake-peer".toByteArray(),
      "10000000-0000-4000-8000-000000000001".toByteArray()) }
    try {
      native { ClientBindings.start(client, 0, null) }
      awaitEvent(client)
      native {
        ClientBindings.bind(client, "20000000-0000-4000-8000-000000000001".toByteArray(), terminal)
        ClientBindings.start(client, 3,
          """{"method":"attachTerminal","payload":{"tabId":"20000000-0000-4000-8000-000000000001"}}""".toByteArray())
      }
      awaitEvent(client)
      native {
        val ticket = ClientBindings.resetTicket(client, terminal)
        val grid = ResetGrid.decode(ClientBindings.resetGrid(client, terminal, ticket))
        require(grid is ResetGrid.Host)
        RustBindings.reset(terminal, grid.cols, grid.rows)
        assertEquals(0, ClientBindings.resetDone(client, terminal, ticket))
        RustBindings.pump(terminal)
        assertTrue(ClientBindings.recoveryReady(client, terminal))
      }
      val nul = native { forward(client,
        """{"type":"key","event":{"physical":63,"action":"press","modifiers":2,"consumedModifiers":0,"text":" ","unshiftedCodepoint":32,"composing":false}}""") }
      assertArrayEquals(byteArrayOf(0), nul.bytes)
      awaitEvent(client)
      val written = requireNotNull(FixtureTransport.writes.poll()).getJSONObject("payload")
      assertEquals("\u0000", written.getString("data"))
      assertTrue(written.getBoolean("userInitiated"))
      native { ClientBindings.start(client, 2, null) }
      awaitEvent(client)
      native { RustBindings.pump(terminal) }
      for (json in listOf(
        """{"type":"key","event":{"physical":78,"action":"release","modifiers":0,"consumedModifiers":0,"text":"","unshiftedCodepoint":null,"composing":false}}""",
        """{"type":"mouse","action":"motion","button":null,"x":16,"y":32,"cellWidth":8,"cellHeight":16,"anyButtonPressed":false}""",
      )) {
        assertFalse(native { forward(client, json) }.userInitiated)
        awaitEvent(client)
        val passive = requireNotNull(FixtureTransport.writes.poll()).getJSONObject("payload")
        assertTrue(passive.has("userInitiated"))
        assertFalse(passive.getBoolean("userInitiated"))
      }
    } finally { native { ClientBindings.close(client, FixtureTransport::class.java) } }
  }
  private fun forward(client: Long, json: String): EncodedInput {
    val encoded = requireNotNull(EncodedInput.decode(RustBindings.inputEx(terminal, json.toByteArray())))
    assertEquals(0, ClientBindings.write(client, terminal, encoded.bytes, encoded.userInitiated))
    return encoded
  }
  private fun awaitEvent(client: Long): JSONObject {
    val deadline = System.nanoTime() + TimeUnit.SECONDS.toNanos(5)
    while (System.nanoTime() < deadline) {
      val bytes = native {
        ClientBindings.dispatch(client, FixtureTransport::class.java)
        ClientBindings.readEvent(client)
      }
      if (bytes != null) return JSONObject(bytes.toString(Charsets.UTF_8)).also { assertEquals(0, it.getInt("status")) }
      Thread.sleep(1)
    }
    error("Native fixture operation timed out")
  }
  @Test fun wrongThreadIsRejectedAndClosedHandlesCannotAcceptLateOutput() {
    val error = assertThrows(NativeCallException::class.java) { RustBindings.resize(terminal, 30, 6) }
    assertEquals(4, error.status)
    val retired = terminal
    native { RustBindings.close(retired) }
    terminal = 0
    assertEquals(3, RustBindings.submit(retired, 1, "late".toByteArray()))
  }
  @Test fun resetClearsIncompleteAnsiAndLocalTicketsOnTheSameOwner() {
    RustBindings.submit(terminal, 1, "\u001b[31".toByteArray())
    native {
      RustBindings.pumpFrame(terminal)
      RustBindings.reset(terminal, 20, 5)
    }
    assertEquals(0L, RustBindings.progress(terminal)[0])
    assertEquals(0, RustBindings.submit(terminal, 1, "clean".toByteArray()))
    assertTrue(native { TerminalFrame.decode(RustBindings.pumpFrame(terminal)) }.text().startsWith("clean"))
  }
}
