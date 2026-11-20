import AppKit
import ApplicationServices
import ImageIO
import ScreenCaptureKit
import UniformTypeIdentifiers
import Vision

private struct ProbeError: Error, CustomStringConvertible {
    let description: String
    init(_ description: String) { self.description = description }
}

private func attribute<T>(_ element: AXUIElement, _ name: String) -> T? {
    var result: CFTypeRef?
    guard AXUIElementCopyAttributeValue(element, name as CFString, &result) == .success else { return nil }
    return result as? T
}

private func rectangle(_ element: AXUIElement) -> CGRect? {
    guard let position: AXValue = attribute(element, kAXPositionAttribute),
          let size: AXValue = attribute(element, kAXSizeAttribute) else { return nil }
    var origin = CGPoint.zero
    var extent = CGSize.zero
    guard AXValueGetValue(position, .cgPoint, &origin), AXValueGetValue(size, .cgSize, &extent) else { return nil }
    return CGRect(origin: origin, size: extent)
}

private func rectJSON(_ rect: CGRect) -> [String: CGFloat] {
    ["x": rect.minX, "y": rect.minY, "width": rect.width, "height": rect.height]
}

private func tree(_ window: AXUIElement) -> [[String: Any]] {
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

private func ownedWindows(_ owner: pid_t) -> [AXUIElement] {
    let app = AXUIElementCreateApplication(owner)
    AXUIElementSetMessagingTimeout(app, 2)
    return attribute(app, kAXWindowsAttribute) ?? []
}

private func ownedWindow(_ owner: pid_t) throws -> AXUIElement {
    let largest = ownedWindows(owner).max { (rectangle($0)?.width ?? 0) < (rectangle($1)?.width ?? 0) }
    guard let window = largest, (rectangle(window)?.width ?? 0) >= 600 else {
        throw ProbeError("No desktop window for owned PID \(owner)")
    }
    return window
}

private func pressButton(_ owner: pid_t, _ name: String) throws {
    var queue = ownedWindows(owner)
    var visited = 0
    while let element = queue.popLast(), visited < 1500 {
        visited += 1
        let role: String? = attribute(element, kAXRoleAttribute)
        let title: String? = attribute(element, kAXTitleAttribute)
        let description: String? = attribute(element, kAXDescriptionAttribute)
        if role == kAXButtonRole, title == name || description == name {
            if AXUIElementPerformAction(element, kAXPressAction as CFString) == .success { return }
        }
        let children: [AXUIElement] = attribute(element, kAXChildrenAttribute) ?? []
        queue.append(contentsOf: children)
    }
    throw ProbeError("Could not press \(name) in an owned native dialog")
}

private func number(_ string: String) throws -> CGFloat {
    guard let value = Double(string), value.isFinite else { throw ProbeError("Invalid number \(string)") }
    return CGFloat(value)
}

private func resize(_ window: AXUIElement, _ args: ArraySlice<String>) throws {
    let values = try args.map(number)
    guard values.count == 4 else { throw ProbeError("Expected x y width height") }
    var position = CGPoint(x: values[0], y: values[1])
    var size = CGSize(width: values[2], height: values[3])
    guard let point = AXValueCreate(.cgPoint, &position), let extent = AXValueCreate(.cgSize, &size),
          AXUIElementSetAttributeValue(window, kAXPositionAttribute as CFString, point) == .success,
          AXUIElementSetAttributeValue(window, kAXSizeAttribute as CFString, extent) == .success else {
        throw ProbeError("Could not resize the owned window through AX")
    }
}

private func key(_ owner: pid_t, _ args: ArraySlice<String>) throws {
    guard let key = args.first, let code = UInt16(key), let modifiers = args.dropFirst().first else {
        throw ProbeError("Expected the virtual key code and modifiers")
    }
    for down in [true, false] {
        guard let event = CGEvent(keyboardEventSource: nil, virtualKey: code, keyDown: down) else {
            throw ProbeError("Could not create keyboard event")
        }
        if modifiers.contains("c") { event.flags.insert(.maskCommand) }
        if modifiers.contains("s") { event.flags.insert(.maskShift) }
        event.postToPid(owner)
    }
}

private func click(_ owner: pid_t, _ args: ArraySlice<String>, _ window: AXUIElement) throws {
    let values = try args.map(number)
    guard values.count == 2 else { throw ProbeError("Expected x y") }
    let point = CGPoint(x: values[0], y: values[1])
    guard rectangle(window)?.contains(point) == true else {
        throw ProbeError("Click falls outside the owned window")
    }
    for type in [CGEventType.leftMouseDown, .leftMouseUp] {
        guard let event = CGEvent(mouseEventSource: nil, mouseType: type, mouseCursorPosition: point, mouseButton: .left) else {
            throw ProbeError("Could not create mouse event")
        }
        event.postToPid(owner)
    }
}

private func colors(_ image: CGImage) throws -> [String: [String: CGFloat]] {
    let targets = [("style", 20, 120, 40), ("asset", 65, 19, 211)]
    let width = image.width, height = image.height
    var pixels = [UInt8](repeating: 0, count: width * height * 4)
    return try pixels.withUnsafeMutableBytes { buffer in
        guard let space = CGColorSpace(name: CGColorSpace.sRGB),
              let context = CGContext(data: buffer.baseAddress, width: width, height: height, bitsPerComponent: 8,
                                      bytesPerRow: width * 4, space: space,
                                      bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue | CGBitmapInfo.byteOrder32Big.rawValue) else {
            throw ProbeError("Could not decode screenshot colors")
        }
        context.draw(image, in: CGRect(x: 0, y: 0, width: CGFloat(width), height: CGFloat(height)))
        var result = [String: [String: CGFloat]]()
        for (name, red, green, blue) in targets {
            var bounds = CGRect.null
            for i in stride(from: 0, to: width * height * 4, by: 4) {
                if abs(Int(buffer[i]) - red) <= 3 && abs(Int(buffer[i + 1]) - green) <= 3 && abs(Int(buffer[i + 2]) - blue) <= 3 {
                    bounds = bounds.union(CGRect(x: CGFloat(i / 4 % width), y: CGFloat(i / 4 / width), width: 1, height: 1))
                }
            }
            if !bounds.isNull { result[name] = rectJSON(bounds) }
        }
        return result
    }
}

@MainActor
private func capture(_ owner: pid_t, _ target: String, _ area: CGRect) async throws -> [String: Any] {
    let content = try await SCShareableContent.excludingDesktopWindows(true, onScreenWindowsOnly: true)
    guard let window = content.windows.first(where: {
        $0.owningApplication?.processID == owner && abs($0.frame.width - area.width) < 4 && abs($0.frame.height - area.height) < 4
    }) else { throw ProbeError("ScreenCaptureKit cannot see the owned window") }
    let filter = SCContentFilter(desktopIndependentWindow: window)
    let config = SCStreamConfiguration()
    let scale = CGFloat(filter.pointPixelScale)
    config.width = Int(window.frame.width * scale)
    config.height = Int(window.frame.height * scale)
    config.showsCursor = false
    config.ignoreShadowsSingleWindow = true
    let image = try await SCScreenshotManager.captureImage(contentFilter: filter, configuration: config)
    guard image.width == config.width, image.height == config.height else {
        throw ProbeError("Captured unexpected pixel bounds")
    }
    let url = URL(fileURLWithPath: target)
    guard let png = CGImageDestinationCreateWithURL(url as CFURL, UTType.png.identifier as CFString, 1, nil) else {
        throw ProbeError("Could not open \(target)")
    }
    CGImageDestinationAddImage(png, image, nil)
    guard CGImageDestinationFinalize(png) else { throw ProbeError("Could not write \(target)") }
    let request = VNRecognizeTextRequest()
    request.recognitionLevel = .accurate
    try VNImageRequestHandler(cgImage: image).perform([request])
    let lines: [[String: Any]] = (request.results ?? []).compactMap { observation in
        guard let text = observation.topCandidates(1).first?.string else { return nil }
        let bounds = observation.boundingBox
        return ["text": text, "x": window.frame.minX + bounds.midX * window.frame.width,
                "y": window.frame.minY + (1 - bounds.midY) * window.frame.height]
    }
    return ["scale": scale, "frame": rectJSON(window.frame), "pixelWidth": image.width,
            "pixelHeight": image.height, "windowCount": content.windows.filter { $0.owningApplication?.processID == owner }.count,
            "lines": lines, "colors": try colors(image)]
}

@MainActor
private func aquaProbe(_ app: NSApplication, _ output: String) async throws {
    let window = NSWindow(contentRect: CGRect(x: 45, y: 45, width: 640, height: 220),
                          styleMask: [.titled], backing: .buffered, defer: false)
    window.title = "Shellfox P4 fixture preflight"
    window.makeKeyAndOrderFront(nil)
    app.activate(ignoringOtherApps: true)
    defer { window.orderOut(nil) }
    let owner = ProcessInfo.processInfo.processIdentifier
    try await Task.sleep(for: .milliseconds(350))
    _ = try await capture(owner, output, window.frame)
    try key(owner, ["111", "-"][...]) // F12, posted only to this probe.
    guard receivesPreflightKey(app) else { throw ProbeError("Aqua did not deliver PID-targeted keyboard input to the owned probe window.") }
}

@MainActor
private func receivesPreflightKey(_ app: NSApplication) -> Bool {
    let stop = Date().addingTimeInterval(2)
    while let event = app.nextEvent(matching: .keyDown, until: stop, inMode: .default, dequeue: true) {
        if event.keyCode == 111 { return true }
    }
    return false
}

@main
private struct WindowProbe {
    @MainActor static func main() async {
        do {
            // ScreenCaptureKit and Quartz expect a WindowServer connection in a CLI probe too.
            let application = NSApplication.shared
            application.setActivationPolicy(.accessory)
            guard AXIsProcessTrusted(), CGPreflightScreenCaptureAccess(),
                  let session = CGSessionCopyCurrentDictionary() as? [String: Any],
                  session[kCGSessionOnConsoleKey as String] as? Bool == true else {
                throw ProbeError("P4 requires a logged-in Aqua session with Accessibility and Screen Recording permissions. Use an isolated graphical runner with those permissions.")
            }
            let args = CommandLine.arguments
            if args.count == 3 && args[1] == "preflight" {
                try await aquaProbe(application, args[2]); return
            }
            guard args.count >= 3, let owner = pid_t(args[1]), owner > 1, let app = NSRunningApplication(processIdentifier: owner) else {
                throw ProbeError("Expected an owned GUI PID and action")
            }
            let window = try ownedWindow(owner)
            let values = args.dropFirst(3)
            let result: Any
            switch args[2] {
            case "tree": result = tree(window)
            case "windows": result = ownedWindows(owner).flatMap(tree)
            case "resize": try resize(window, values); return
            case "activate":
                guard app.activate(options: []) else { throw ProbeError("Could not activate the owned app") }
                return
            case "key": try key(owner, values); return
            case "press":
                guard let name = values.first else { throw ProbeError("Missing native button name") }
                try pressButton(owner, name); return
            case "paste":
                guard let text = values.first else { throw ProbeError("Missing clipboard fixture") }
                let pasteboard = NSPasteboard.general
                pasteboard.clearContents()
                guard pasteboard.setString(text, forType: .string) else { throw ProbeError("Could not set the fixture clipboard") }
                try key(owner, ["9", "c"][...]); return
            case "clipboard": result = ["value": NSPasteboard.general.string(forType: .string) ?? ""]
            case "click": try click(owner, values, window); return
            case "capture":
                guard let target = values.first, let bounds = rectangle(window) else {
                    throw ProbeError("Missing screenshot path or owned window bounds")
                }
                result = try await capture(owner, target, bounds)
            default: throw ProbeError("Unknown window probe action: \(args[2])")
            }
            FileHandle.standardOutput.write(try JSONSerialization.data(withJSONObject: result))
        } catch {
            FileHandle.standardError.write(Data("\(error)\n".utf8))
            exit(1)
        }
    }
}
