package com.shellfox.mobile.rust

import com.facebook.react.uimanager.SimpleViewManager
import com.facebook.react.uimanager.ThemedReactContext
import com.facebook.react.uimanager.ViewManagerDelegate
import com.facebook.react.uimanager.annotations.ReactProp
import com.facebook.react.viewmanagers.ShellfoxRustTerminalManagerDelegate
import com.facebook.react.viewmanagers.ShellfoxRustTerminalManagerInterface

internal class ShellfoxRustTerminalManager : SimpleViewManager<ShellfoxRustTerminalView>(),
  ShellfoxRustTerminalManagerInterface<ShellfoxRustTerminalView> {
  private val generatedDelegate = ShellfoxRustTerminalManagerDelegate(this)
  override fun getDelegate(): ViewManagerDelegate<ShellfoxRustTerminalView> = generatedDelegate
  override fun getName() = "ShellfoxRustTerminal"
  override fun createViewInstance(context: ThemedReactContext) = ShellfoxRustTerminalView(context)
  @ReactProp(name = "terminalHandle")
  override fun setTerminalHandle(view: ShellfoxRustTerminalView, value: String?) = view.setTerminalHandle(value)
  @ReactProp(name = "readOnly", defaultBoolean = false)
  override fun setReadOnly(view: ShellfoxRustTerminalView, value: Boolean) { view.readOnly = value }
  @ReactProp(name = "focused", defaultBoolean = false)
  override fun setFocused(view: ShellfoxRustTerminalView, value: Boolean) = view.setFocused(value)
  @ReactProp(name = "selecting", defaultBoolean = false)
  override fun setSelecting(view: ShellfoxRustTerminalView, value: Boolean) = view.setSelecting(value)
  @ReactProp(name = "modifiers", defaultInt = 0)
  override fun setModifiers(view: ShellfoxRustTerminalView, value: Int) = view.setModifiers(value)
  @ReactProp(name = "fontSize", defaultFloat = 14f)
  override fun setFontSize(view: ShellfoxRustTerminalView, value: Float) = view.setFontSize(value)
  override fun onDropViewInstance(view: ShellfoxRustTerminalView) { view.dispose(); super.onDropViewInstance(view) }
  override fun getExportedCustomDirectEventTypeConstants(): Map<String, Any> = mapOf(
    "topInput" to mapOf("registrationName" to "onInput"),
    "topSelection" to mapOf("registrationName" to "onSelection"),
    "topFileLink" to mapOf("registrationName" to "onFileLink"),
    "topState" to mapOf("registrationName" to "onState"),
    "topError" to mapOf("registrationName" to "onError"),
  )
}
