package com.shellfox.mobile.rust

import android.os.Handler
import android.os.HandlerThread
import androidx.test.ext.junit.runners.AndroidJUnit4
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import java.util.concurrent.FutureTask
import java.util.concurrent.TimeUnit

/** Actual linked Manager JNI factory/owner gates. No Go node or server grant. */
@RunWith(AndroidJUnit4::class)
class ManagerJniFixtureTest {
  @Test fun explicitManagerFactoryAndTwoOwnerViewsNeverTurnIntoLegacyClient() {
    val thread=HandlerThread("ManagerJniFixtureOwner").apply { start() }
    val owner=Handler(thread.looper)
    fun <T> native(block:()->T):T { val task=FutureTask(block);owner.post(task);return task.get(5,TimeUnit.SECONDS) }
    var client=0L;var first=0L;var second=0L
    try {
      native {
        assertEquals(1,ClientBindings.managerAbiVersion())
        client=ClientBindings.createManager("n-fake-peer".toByteArray(),"10000000-0000-4000-8000-000000000001".toByteArray())
        assertArrayEquals(intArrayOf(3,7),ClientBindings.profile(client))
        first=RustBindings.create(20,5);second=RustBindings.create(20,5)
        ClientBindings.bind(client,"20000000-0000-4000-8000-000000000001".toByteArray(),first)
        ClientBindings.bind(client,"30000000-0000-4000-8000-000000000001".toByteArray(),second)
        assertFalse(ClientBindings.recoveryReady(client,first))
        assertFalse(ClientBindings.recoveryReady(client,second))
        assertEquals(0L,ClientBindings.resetTicket(client,first))
        val noLegacy=assertThrows(NativeCallException::class.java){ClientBindings.start(client,0,null)}
        assertEquals(2,noLegacy.status)
        ClientBindings.unbind(client,first)
        assertArrayEquals(intArrayOf(3,7),ClientBindings.profile(client))
        assertFalse(ClientBindings.recoveryReady(client,second))
      }
      val noLocalFit=assertThrows(NativeCallException::class.java){RustBindings.resize(second,30,6)}
      assertEquals(2,noLocalFit.status) // Manager rejects parser fitting before owner dispatch.
      val wrong=assertThrows(NativeCallException::class.java){RustBindings.pumpFrame(second)}
      assertEquals(4,wrong.status)
    } finally {
      native {
        if(client!=0L){if(second!=0L)ClientBindings.unbind(client,second);ClientBindings.close(client,FixtureTransport::class.java)}
        if(first!=0L)RustBindings.close(first)
        if(second!=0L)RustBindings.close(second)
      }
      thread.quitSafely();thread.join(5000)
    }
  }
}
