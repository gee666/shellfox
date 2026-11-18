package com.shellfox.mobile

import com.facebook.react.BaseReactPackage
import com.facebook.react.bridge.NativeModule
import com.facebook.react.bridge.ReactApplicationContext
import com.facebook.react.module.model.ReactModuleInfo
import com.facebook.react.module.model.ReactModuleInfoProvider
import com.facebook.react.uimanager.ViewManager
import com.shellfox.mobile.rust.ShellfoxRustTerminalManager

class ShellfoxRustPackage : BaseReactPackage() {
  override fun getModule(name: String, reactContext: ReactApplicationContext): NativeModule? =
    if (name == NativeShellfoxRustClientSpec.NAME) ShellfoxRustClientModule(reactContext) else null
  override fun getReactModuleInfoProvider() = ReactModuleInfoProvider {
    mapOf(NativeShellfoxRustClientSpec.NAME to ReactModuleInfo(
      NativeShellfoxRustClientSpec.NAME, ShellfoxRustClientModule::class.java.name,
      false, false, false, true,
    ))
  }
  override fun createViewManagers(reactContext: ReactApplicationContext): List<ViewManager<*, *>> =
    listOf(ShellfoxRustTerminalManager())
}
