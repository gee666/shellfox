package com.shellfox.mobile.rust

import androidx.test.ext.junit.runners.AndroidJUnit4
import org.json.JSONObject
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith
import java.util.concurrent.CompletableFuture
import java.util.concurrent.TimeUnit

/**
 * Reopening a session recovers its screen before the new view has measured
 * itself. The recovered engine must keep the host grid until then, not fit
 * the create placeholder: that pushes rows above a parked cursor into
 * scrollback and leaves a TUI footer mid-screen once the view grows it back.
 */
@RunWith(AndroidJUnit4::class)
class TerminalRecoveryFitTest {
  private val module = Any()
  private lateinit var id: String
  private val sink = object : TerminalCoordinator.TerminalSink {
    override fun resetInput(epoch: Long) = Unit
    override fun typedInput(json: JSONObject) = Unit
    override fun userInput() = Unit
    override fun appearance(appearance: TerminalAppearance) = Unit
    override fun frame(frame: TerminalFrame) = Unit
    override fun state(state: String) = Unit
    override fun error(code: Int) = Unit
  }
  private fun <T> owner(action: () -> T): T {
    val result = CompletableFuture<T>()
    TerminalCoordinator.execute({ result.completeExceptionally(IllegalStateException("busy")) }) {
      try { result.complete(action()) } catch (error: Throwable) { result.completeExceptionally(error) }
    }
    return result.get(5, TimeUnit.SECONDS)
  }
  private fun footerRows(): Pair<Pair<Int, Int>, List<Int>> = owner {
    val frame = TerminalFrame.decode(RustBindings.pumpFrame(TerminalCoordinator.ownedHandle(module, id)))
    Pair(Pair(frame.cols, frame.rows), frame.text().lines().withIndex().filter { it.value.startsWith("FOOTER") }.map { it.index })
  }

  @Before fun create() {
    val created = CompletableFuture<String>()
    TerminalCoordinator.create(module, 80.0, 24.0) { value, error ->
      if (value != null) created.complete(value) else created.completeExceptionally(IllegalStateException(error))
    }
    id = created.get(5, TimeUnit.SECONDS)
    val history = (0 until 60).joinToString("") { "line $it\r\n" }
    owner {
      TerminalCoordinator.reset(id, 60, 50)
      val handle = TerminalCoordinator.ownedHandle(module, id)
      RustBindings.submit(handle, 1, "${history}editor\r\n\r\nFOOTER-1\r\nFOOTER-2\u001b[3A\r".toByteArray())
      RustBindings.pump(handle)
      TerminalCoordinator.fitAfterRecovery(id)
    }
  }
  @After fun close() {
    TerminalCoordinator.detach(id, sink)
    val closed = CompletableFuture<String?>()
    TerminalCoordinator.close(module, id) { closed.complete(it) }
    closed.get(5, TimeUnit.SECONDS)
  }

  @Test fun recoveryBeforeTheViewMeasuresKeepsTheHostGrid() {
    assertEquals(Pair(Pair(60, 50), listOf(48, 49)), footerRows())
  }
  @Test fun theViewsFirstResizeFitsTheRecoveredScreen() {
    TerminalCoordinator.attach(id, sink)
    TerminalCoordinator.resize(id, sink, 60, 40, false)
    assertEquals(Pair(Pair(60, 40), listOf(38, 39)), footerRows())
  }
}
