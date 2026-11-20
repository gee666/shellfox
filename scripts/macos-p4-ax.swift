import AppKit
import ApplicationServices

func attribute<T>(_ element: AXUIElement, _ name: String) -> T? {
    var result: CFTypeRef?
    guard AXUIElementCopyAttributeValue(element, name as CFString, &result) == .success else { return nil }
    return result as? T
}

func rectangle(_ element: AXUIElement) -> CGRect? {
    guard let position: AXValue = attribute(element, kAXPositionAttribute),
          let size: AXValue = attribute(element, kAXSizeAttribute) else { return nil }
    var origin = CGPoint.zero
    var extent = CGSize.zero
    guard AXValueGetValue(position, .cgPoint, &origin), AXValueGetValue(size, .cgSize, &extent) else { return nil }
    return CGRect(origin: origin, size: extent)
}

func rectJSON(_ rect: CGRect) -> [String: CGFloat] {
    ["x": rect.minX, "y": rect.minY, "width": rect.width, "height": rect.height]
}

func tree(_ window: AXUIElement) -> [[String: Any]] {
    var queue = [window]
    var result = [[String: Any]]()
    while let element = queue.popLast(), result.count < 1500 {
        var row = [String: Any]()
        for (key, name) in [("role", kAXRoleAttribute), ("subrole", kAXSubroleAttribute), ("title", kAXTitleAttribute),
                            ("description", kAXDescriptionAttribute), ("help", kAXHelpAttribute), ("value", kAXValueAttribute)] {
            if let value: String = attribute(element, name) { row[key] = value }
        }
        if let rect = rectangle(element) { row["rect"] = rectJSON(rect) }
        if let focused: Bool = attribute(element, kAXFocusedAttribute) { row["focused"] = focused }
        if let enabled: Bool = attribute(element, kAXEnabledAttribute) { row["enabled"] = enabled }
        result.append(row)
        let children: [AXUIElement] = attribute(element, kAXChildrenAttribute) ?? []
        queue.append(contentsOf: children.reversed())
    }
    return result
}

func ownedWindows(_ owner: pid_t) -> [AXUIElement] {
    let app = AXUIElementCreateApplication(owner)
    AXUIElementSetMessagingTimeout(app, 2)
    return attribute(app, kAXWindowsAttribute) ?? []
}

func ownedWindow(_ owner: pid_t) throws -> AXUIElement {
    let largest = ownedWindows(owner).max { (rectangle($0)?.width ?? 0) < (rectangle($1)?.width ?? 0) }
    guard let window = largest, (rectangle(window)?.width ?? 0) >= 600 else {
        throw ProbeError("No desktop window for owned PID \(owner)")
    }
    return window
}

func nativeDialogs(_ owner: pid_t) -> [AXUIElement] {
    var visited = 0
    var dialogs = [AXUIElement]()
    var queue = ownedWindows(owner)
    while let element = queue.popLast(), visited < 1500 {
        visited += 1
        let role: String? = attribute(element, kAXRoleAttribute)
        let subrole: String? = attribute(element, kAXSubroleAttribute)
        if role == "AXSheet" || subrole == "AXDialog" || subrole == "AXSystemDialog" {
            dialogs.append(element)
        }
        let children: [AXUIElement] = attribute(element, kAXChildrenAttribute) ?? []
        queue.append(contentsOf: children.reversed())
    }
    return dialogs
}

func dialogOwner(_ owner: pid_t) throws -> pid_t {
    var modalPID: pid_t = 0
    guard let dialog = nativeDialogs(owner).last,
          AXUIElementGetPid(dialog, &modalPID) == .success, modalPID > 1 else {
        throw ProbeError("No native modal attached to the owned application")
    }
    return modalPID
}

func pressControl(_ owner: pid_t, _ name: String, modal: Bool) throws {
    var queue = modal ? nativeDialogs(owner) : ownedWindows(owner)
    var visited = 0
    while let element = queue.popLast(), visited < 1500 {
        visited += 1
        let role: String? = attribute(element, kAXRoleAttribute)
        let title: String? = attribute(element, kAXTitleAttribute)
        let description: String? = attribute(element, kAXDescriptionAttribute)
        if role == kAXButtonRole || (!modal && role == kAXCheckBoxRole), title == name || description == name {
            if AXUIElementPerformAction(element, kAXPressAction as CFString) == .success { return }
        }
        let children: [AXUIElement] = attribute(element, kAXChildrenAttribute) ?? []
        queue.append(contentsOf: children)
    }
    throw ProbeError("Could not press \(name) in an owned \(modal ? "native dialog" : "window")")
}

func number(_ string: String) throws -> CGFloat {
    guard let value = Double(string), value.isFinite else { throw ProbeError("Invalid number \(string)") }
    return CGFloat(value)
}

let regularChrome: NSWindow.StyleMask = [.titled, .closable, .resizable, .miniaturizable]

@MainActor
func resizeContent(_ window: AXUIElement, _ args: ArraySlice<String>) throws {
    let values = try args.map(number)
    guard values.count == 4 else { throw ProbeError("Expected x y width height") }
    var position = CGPoint(x: values[0], y: values[1])
    var size = NSWindow.frameRect(forContentRect: CGRect(x: 0, y: 0, width: values[2], height: values[3]),
                                  styleMask: regularChrome).size
    guard let point = AXValueCreate(.cgPoint, &position), let extent = AXValueCreate(.cgSize, &size),
          AXUIElementSetAttributeValue(window, kAXPositionAttribute as CFString, point) == .success,
          AXUIElementSetAttributeValue(window, kAXSizeAttribute as CFString, extent) == .success else {
        throw ProbeError("Could not resize the owned window through AX")
    }
}

func key(_ owner: pid_t, _ args: ArraySlice<String>) throws {
    guard let key = args.first, let code = UInt16(key), let modifiers = args.dropFirst().first else {
        throw ProbeError("Expected the virtual key code and modifiers")
    }
    let source = CGEventSource(stateID: .hidSystemState)
    for down in [true, false] {
        guard let event = CGEvent(keyboardEventSource: source, virtualKey: code, keyDown: down) else {
            throw ProbeError("Could not create keyboard event")
        }
        if modifiers.contains("c") { event.flags.insert(.maskCommand) }
        if modifiers.contains("s") { event.flags.insert(.maskShift) }
        event.postToPid(owner)
    }
}

func click(_ owner: pid_t, _ args: ArraySlice<String>, _ window: AXUIElement) throws {
    let values = try args.map(number)
    guard values.count == 2 else { throw ProbeError("Expected x y") }
    let point = CGPoint(x: values[0], y: values[1])
    guard rectangle(window)?.contains(point) == true else {
        throw ProbeError("Click falls outside the owned window")
    }
    let windows = CGWindowListCopyWindowInfo([.optionOnScreenOnly, .excludeDesktopElements], kCGNullWindowID) as? [[String: Any]] ?? []
    let target = windows.first { entry in
        guard entry[kCGWindowOwnerPID as String] as? pid_t == owner,
              let bounds = entry[kCGWindowBounds as String] as? [String: Any],
              let frame = CGRect(dictionaryRepresentation: bounds as CFDictionary) else { return false }
        return frame.contains(point)
    }
    guard let number = target?[kCGWindowNumber as String] as? NSNumber else {
        throw ProbeError("Quartz did not find an owned native window under the requested click")
    }
    let source = CGEventSource(stateID: .hidSystemState)
    for type in [CGEventType.mouseMoved, .leftMouseDown, .leftMouseUp] {
        guard let event = CGEvent(mouseEventSource: source, mouseType: type, mouseCursorPosition: point, mouseButton: .left) else {
            throw ProbeError("Could not create mouse event")
        }
        event.setIntegerValueField(.mouseEventWindowUnderMousePointer, value: number.int64Value)
        event.setIntegerValueField(.mouseEventWindowUnderMousePointerThatCanHandleThisEvent, value: number.int64Value)
        if type != .mouseMoved { event.setIntegerValueField(.mouseEventClickState, value: 1) }
        event.postToPid(owner)
    }
}
