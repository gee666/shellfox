package com.shellfox.mobile.rust

import android.content.Intent
import android.view.KeyEvent
import android.view.ViewGroup
import android.view.inputmethod.EditorInfo
import android.view.inputmethod.InputConnection
import androidx.core.view.ViewCompat
import androidx.core.view.WindowInsetsCompat
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import com.shellfox.mobile.MainActivity
import org.json.JSONObject
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import kotlin.concurrent.thread

/**
 * The real terminal view, owner thread, Rust client and soft keyboard in the
 * app's activity, against the fixture host. Does what JS does: polls, and
 * reattaches when native asks.
 */
@RunWith(AndroidJUnit4::class)
class TerminalViewHostTest {
  private val instrumentation = InstrumentationRegistry.getInstrumentation()
  private val module = object : ClientEventSink { override fun clientEvent(clientHandle: String) = Unit }
  private val tab = "20000000-0000-4000-8000-000000000001"
  private lateinit var activity: MainActivity
  private lateinit var view: ShellfoxRustTerminalView
  private lateinit var client: String
  @Volatile private var running = true
  private lateinit var pump: Thread

  private fun <T> owner(action: () -> T): T {
    var value: Result<T>? = null
    val latch = CountDownLatch(1)
    TerminalCoordinator.execute({ error("owner queue full") }) { value = runCatching(action); latch.countDown() }
    check(latch.await(10, TimeUnit.SECONDS))
    return value!!.getOrThrow()
  }
  private fun attach(request: JSONObject) = owner {
    ClientTerminalBindings.afterPoll(module, client, {
      ClientTerminalBindings.start(module, client, 3, JSONObject().put("method", "attachTerminal").put("payload", request).toString())
    }) { _, _ -> }
  }
  private fun waitFor(what: String, seconds: Long = 10, condition: () -> Boolean) {
    val deadline = System.nanoTime() + TimeUnit.SECONDS.toNanos(seconds)
    while (!condition()) { check(System.nanoTime() < deadline) { "Timed out: $what" }; Thread.sleep(20) }
  }
  private fun imeVisible(): Boolean {
    var visible = false
    instrumentation.runOnMainSync {
      visible = ViewCompat.getRootWindowInsets(view)?.isVisible(WindowInsetsCompat.Type.ime()) == true
    }
    return visible
  }

  @Before fun open() {
    val assets = instrumentation.context.assets
    FixtureTransport.initialize(assets.open("remote-pair-success.json").use { it.readBytes() },
      assets.open("terminal-attach-owned-success.json").use { it.readBytes() })
    ClientTerminalBindings.transport = FixtureTransport::class.java
    activity = instrumentation.startActivitySync(Intent(instrumentation.targetContext, MainActivity::class.java)
      .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)) as MainActivity
    // A debug build without Metro shows React Native's red box; it has focus.
    Thread.sleep(2000)
    instrumentation.sendKeyDownUpSync(KeyEvent.KEYCODE_ESCAPE)
    client = owner { ClientTerminalBindings.create(module, "n-fake-peer", "10000000-0000-4000-8000-000000000001") }
    val connected = CountDownLatch(1)
    pump = thread {
      var polling = false
      var connecting = true
      while (running) {
        val event = runCatching { owner { ClientTerminalBindings.readEvent(module, client) } }.getOrNull()?.let(::JSONObject)
        if (event == null) {
          if (!connecting && !polling) polling = runCatching { owner { ClientTerminalBindings.start(module, client, 2) } }.isSuccess
          Thread.sleep(30); continue
        }
        if (connecting) { connecting = false; connected.countDown() } else polling = false
        val reattach = event.optJSONArray("reattach")
        if (reattach != null) for (i in 0 until reattach.length()) attach(reattach.getJSONObject(i))
      }
    }
    owner { ClientTerminalBindings.start(module, client, 0) }
    check(connected.await(10, TimeUnit.SECONDS))
    val created = CountDownLatch(1)
    var terminal = ""
    TerminalCoordinator.create(module, 80.0, 24.0) { id, error -> terminal = id ?: error("create $error"); created.countDown() }
    check(created.await(10, TimeUnit.SECONDS))
    val bound = CountDownLatch(1)
    owner { ClientTerminalBindings.changeViewBinding(module, client, tab, terminal) { _, _ -> bound.countDown() } }
    check(bound.await(10, TimeUnit.SECONDS))
    attach(JSONObject().put("tabId", tab))
    instrumentation.runOnMainSync {
      view = ShellfoxRustTerminalView(activity)
      // RustTerminalView pads its native view by the keyboard's overlap; so does this frame.
      val frame = android.widget.FrameLayout(activity).apply { addView(view) }
      ViewCompat.setOnApplyWindowInsetsListener(frame) { padded, insets ->
        padded.setPadding(0, 0, 0, insets.getInsets(WindowInsetsCompat.Type.ime()).bottom); insets
      }
      activity.addContentView(frame, ViewGroup.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT))
      view.setTerminalHandle(terminal)
    }
    waitFor("first resize") { FixtureTransport.resizes.isNotEmpty() }
  }
  @After fun close() {
    running = false
    pump.join(5000)
    owner { ClientTerminalBindings.close(module, client) }
    instrumentation.runOnMainSync { view.dispose(); activity.finish() }
    ClientTerminalBindings.transport = NativeGoTransport::class.java
  }
  private fun rows() = FixtureTransport.resizes.last().getInt("rows")

  @Test fun theHostFollowsTheKeyboardOpeningAndClosing() {
    val full = rows()
    instrumentation.runOnMainSync { view.setFocused(true) }
    waitFor("keyboard shown") { imeVisible() }
    waitFor("rows above the keyboard") { rows() < full }
    instrumentation.runOnMainSync { view.setFocused(false) }
    waitFor("keyboard hidden") { !imeVisible() }
    waitFor("rows restored") { rows() == full }
  }

  @Test fun aRefusedWriteDropsOnlyThatInputAndLostControlReattaches() {
    instrumentation.runOnMainSync { view.setFocused(true) }
    waitFor("terminal focused") { var focused = false; instrumentation.runOnMainSync { focused = view.isFocused }; focused }
    // A fresh connection each time, as the keyboard gets after a reattach.
    fun type(text: String) = instrumentation.runOnMainSync {
      val connection: InputConnection = requireNotNull(view.onCreateInputConnection(EditorInfo()))
      connection.commitText(text, 1); connection.finishComposingText()
    }
    fun typed() = FixtureTransport.writes.joinToString("") { it.getJSONObject("payload").getString("data") }
    FixtureTransport.refuseNextWrite = "VALIDATION"
    type("a")
    type("b")
    waitFor("later input after a refused write") { typed() == "b" }
    FixtureTransport.refuseNextWrite = "TERMINAL_CONTROL_REQUIRED"
    type("c")
    waitFor("a fresh attachment") { typed().isNotEmpty() && run { type("d"); Thread.sleep(200); typed().endsWith("d") } }
    assertTrue(typed(), typed().startsWith("b"))
    assertEquals(false, typed().contains("c"))
  }
}
