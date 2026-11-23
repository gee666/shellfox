package com.shellfox.mobile

import android.os.Bundle
import com.facebook.react.ReactActivity
import com.facebook.react.ReactActivityDelegate
import com.facebook.react.defaults.DefaultNewArchitectureEntryPoint.fabricEnabled
import com.facebook.react.defaults.DefaultReactActivityDelegate

class MainActivity : ReactActivity() {

  /**
   * Returns the name of the main component registered from JavaScript. This is used to schedule
   * rendering of the component.
   */
  override fun getMainComponentName(): String = "ShellfoxMobile"

  /**
   * Returns the instance of the [ReactActivityDelegate]. We use [DefaultReactActivityDelegate]
   * which allows you to enable New Architecture with a single boolean flags [fabricEnabled]
   */
  override fun createReactActivityDelegate(): ReactActivityDelegate =
      object : DefaultReactActivityDelegate(this, mainComponentName, fabricEnabled) {
        override fun getLaunchOptions(): Bundle? {
          if (!BuildConfig.SHELLFOX_LOOPBACK_FIXTURE) return null
          return Bundle().apply {
            intent.getStringExtra("shellfoxControlURL")?.let {
              require(it.toByteArray(Charsets.UTF_8).size <= 2048)
              putString("shellfoxControlURL", it) // The Go transport validates the loopback origin.
            }
          }
        }
      }
}
