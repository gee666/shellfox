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

func tree(_ window: AXUIElement, includeGeometry: Bool = true) -> [[String: Any]] {
    var queue = [window]
    var result = [[String: Any]]()
    let strings = [("role", kAXRoleAttribute), ("subrole", kAXSubroleAttribute), ("title", kAXTitleAttribute),
                   ("description", kAXDescriptionAttribute), ("help", kAXHelpAttribute), ("value", kAXValueAttribute)]
    let fields = strings + [("focused", kAXFocusedAttribute), ("enabled", kAXEnabledAttribute)]
    let names = fields.map { $0.1 } as CFArray
    while let element = queue.popLast(), result.count < 1500 {
        var row = [String: Any]()
        var batch: CFArray?
        if AXUIElementCopyMultipleAttributeValues(element, names, [], &batch) == .success,
           let values = batch as? [Any], values.count == fields.count {
            for ((key, _), raw) in zip(fields, values) {
                if key == "focused" || key == "enabled" { row[key] = raw as? Bool }
                else { row[key] = raw as? String }
            }
        } else {
            for (key, name) in strings {
                if let value: String = attribute(element, name) { row[key] = value }
            }
            if let focused: Bool = attribute(element, kAXFocusedAttribute) { row["focused"] = focused }
            if let enabled: Bool = attribute(element, kAXEnabledAttribute) { row["enabled"] = enabled }
        }
        if includeGeometry, let rect = rectangle(element) { row["rect"] = rectJSON(rect) }
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

func nativeDialogs(_ owner: pid_t, windows: [AXUIElement]? = nil) -> [AXUIElement] {
    var visited = 0
    var dialogs = [AXUIElement]()
    var queue = windows ?? ownedWindows(owner)
    while let element = queue.popLast(), visited < 1500 {
        visited += 1
        let role: String? = attribute(element, kAXRoleAttribute)
        let subrole: String? = role == kAXWindowRole ? attribute(element, kAXSubroleAttribute) : nil
        let title: String? = role == kAXWindowRole ? attribute(element, kAXTitleAttribute) : nil
        let standalonePicker = title.map { ["Open", "Save", "Save As"].contains($0) } ?? false
        if role == "AXSheet" || role == "AXDialog" || subrole == "AXDialog" || subrole == "AXSystemDialog" || standalonePicker {
            dialogs.append(element)
        }
        // File lists and web documents cannot contain AppKit modal windows.
        if let role, ["AXOutline", "AXTable", "AXScrollArea", "AXWebArea"].contains(role) { continue }
        let children: [AXUIElement] = attribute(element, kAXChildrenAttribute) ?? []
        queue.append(contentsOf: children.reversed())
    }
    return dialogs
}

func dialogRows(_ owner: pid_t) throws -> [[String: Any]] {
    let windows = ownedWindows(owner)
    guard !windows.isEmpty else { throw ProbeError("No accessible window from the owned fixture") }
    return nativeDialogs(owner, windows: windows).flatMap { tree($0, includeGeometry: false) }
}

func dialogOwner(_ owner: pid_t) throws -> pid_t {
    var modalPID: pid_t = 0
    guard let dialog = nativeDialogs(owner).last,
          AXUIElementGetPid(dialog, &modalPID) == .success, modalPID > 1 else {
        throw ProbeError("No native modal attached to the owned application")
    }
    return modalPID
}

private func findAX(_ roots: [AXUIElement], _ matches: (AXUIElement) -> Bool) -> AXUIElement? {
    var queue = roots
    var visited = 0
    while let element = queue.popLast(), visited < 1500 {
        visited += 1
        if matches(element) { return element }
        let children: [AXUIElement] = attribute(element, kAXChildrenAttribute) ?? []
        queue.append(contentsOf: children.reversed())
    }
    return nil
}

func editTextControl(_ window: AXUIElement, _ name: String, value: String?) throws {
    guard let entry = findAX([window], { element in
        let role: String? = attribute(element, kAXRoleAttribute)
        let title: String? = attribute(element, kAXTitleAttribute)
        return (role == kAXTextFieldRole || role == kAXTextAreaRole) && title == name
    }), AXUIElementSetAttributeValue(entry, kAXFocusedAttribute as CFString, kCFBooleanTrue) == .success else {
        throw ProbeError("Could not focus \(name) in the owned window")
    }
    if let value, AXUIElementSetAttributeValue(entry, kAXValueAttribute as CFString, value as CFString) != .success {
        throw ProbeError("Could not set \(name) in the owned window")
    }
}

@MainActor
func selectDialogEntry(_ owner: pid_t, _ name: String) throws {
    let dialogs = nativeDialogs(owner)
    guard let entry = findAX(dialogs, { element in
        [kAXTitleAttribute, kAXDescriptionAttribute, kAXValueAttribute].contains { key in
            let text: String? = attribute(element, key)
            return text == name
        }
    }), let row = rectangle(entry), !row.isEmpty, let dialog = dialogs.last,
    let bounds = rectangle(dialog) else {
        throw ProbeError("No \(name) in the owned native picker")
    }
    // Some AppKit rows acknowledge AXSelected without changing Open's state.
    // Hit the actual file label after making its bounding box visible instead.
    var target = row
    if !bounds.contains(CGPoint(x: target.midX, y: target.midY)) {
        // AppKit column view exposes offscreen file labels. Scroll the native
        // file browser to its current directory, not the application's view.
        _ = AXUIElementPerformAction(entry, "AXScrollToVisible" as CFString)
        if let bar = findAX([dialog], { element in
            let role: String? = attribute(element, kAXRoleAttribute)
            guard role == kAXScrollBarRole, let rect = rectangle(element) else { return false }
            return rect.width > rect.height && bounds.intersects(rect)
        }) {
            _ = AXUIElementSetAttributeValue(bar, kAXValueAttribute as CFString, NSNumber(value: 1.0))
        }
        for _ in 0..<12 {
            if let frame = rectangle(entry) { target = frame }
            if bounds.contains(CGPoint(x: target.midX, y: target.midY)) { break }
            Thread.sleep(forTimeInterval: 0.05)
        }
    }
    let pid = try dialogOwner(owner)
    try click(owner, [String(Double(target.midX)), String(Double(target.midY))][...], dialog, modal: pid)
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

@MainActor
func requireFrontmost(_ owners: [pid_t]) throws {
    guard let front = NSWorkspace.shared.frontmostApplication, owners.contains(front.processIdentifier) else {
        throw ProbeError("The fixture or its native dialog must have keyboard focus before posting input")
    }
}

func key(_ owner: pid_t, _ args: ArraySlice<String>, throughWindowServer: Bool = false) throws {
    guard let key = args.first, let code = UInt16(key), let modifiers = args.dropFirst().first else {
        throw ProbeError("Expected the virtual key code and modifiers")
    }
    let source = CGEventSource(stateID: .hidSystemState)
    var chord: [(UInt16, CGEventFlags)] = []
    if modifiers.contains("c") { chord.append((55, .maskCommand)) }
    if modifiers.contains("s") { chord.append((56, .maskShift)) }
    var flags: CGEventFlags = []
    func post(_ virtualKey: UInt16, down: Bool, modifier: Bool = false) throws {
        guard let event = CGEvent(keyboardEventSource: source, virtualKey: virtualKey, keyDown: down) else {
            throw ProbeError("Could not create keyboard event")
        }
        if modifier { event.type = .flagsChanged }
        event.flags = event.flags.subtracting([.maskCommand, .maskShift, .maskControl, .maskAlternate, .maskAlphaShift]).union(flags)
        if throughWindowServer { event.post(tap: .cghidEventTap) }
        else { event.postToPid(owner) }
    }
    defer {
        for (modifier, mask) in chord.reversed() {
            flags.remove(mask)
            try? post(modifier, down: false, modifier: true)
        }
    }
    for (modifier, mask) in chord {
        flags.insert(mask)
        try post(modifier, down: true, modifier: true)
    }
    try post(code, down: true)
    try post(code, down: false)
}

@MainActor
func click(_ owner: pid_t, _ args: ArraySlice<String>, _ window: AXUIElement, modal: pid_t? = nil) throws {
    let values = try args.map(number)
    guard values.count == 2 else { throw ProbeError("Expected x y") }
    let point = CGPoint(x: values[0], y: values[1])
    guard rectangle(window)?.contains(point) == true else {
        throw ProbeError("Click falls outside the owned window")
    }
    let windows = CGWindowListCopyWindowInfo([.optionOnScreenOnly, .excludeDesktopElements], kCGNullWindowID) as? [[String: Any]] ?? []
    let target = windows.first { entry in
        guard entry[kCGWindowLayer as String] as? Int == 0,
              let bounds = entry[kCGWindowBounds as String] as? [String: Any],
              let frame = CGRect(dictionaryRepresentation: bounds as CFDictionary) else { return false }
        return frame.contains(point)
    }
    let allowed = [owner] + (modal.map { [$0] } ?? [])
    guard let targetPID = target?[kCGWindowOwnerPID as String] as? pid_t, allowed.contains(targetPID) else {
        throw ProbeError("The owned fixture is not the topmost window at the click position")
    }
    try requireFrontmost(allowed)
    let source = CGEventSource(stateID: .hidSystemState)
    // Let WindowServer translate screen coordinates into AppKit window coordinates.
    for type in [CGEventType.mouseMoved, .leftMouseDown, .leftMouseUp] {
        guard let event = CGEvent(mouseEventSource: source, mouseType: type, mouseCursorPosition: point, mouseButton: .left) else {
            throw ProbeError("Could not create mouse event")
        }
        if type != .mouseMoved { event.setIntegerValueField(.mouseEventClickState, value: 1) }
        event.post(tap: .cghidEventTap)
        if type != .leftMouseUp { Thread.sleep(forTimeInterval: 0.04) }
    }
}
