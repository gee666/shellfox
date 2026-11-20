import AppKit
import ApplicationServices
import ImageIO
import ScreenCaptureKit
import UniformTypeIdentifiers
import Vision

struct ProbeError: Error, CustomStringConvertible {
    let description: String
    init(_ description: String) { self.description = description }
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
    let contentFrame = NSWindow.contentRect(forFrameRect: window.frame, styleMask: regularChrome)
    return ["scale": scale, "frame": rectJSON(window.frame), "contentSize": [contentFrame.width, contentFrame.height], "pixelWidth": image.width,
            "pixelHeight": image.height, "windowCount": content.windows.filter { $0.owningApplication?.processID == owner }.count,
            "lines": lines, "colors": try colors(image)]
}

@MainActor
private func aquaProbe(_ app: NSApplication, _ output: String) async throws {
    guard ProcessInfo.processInfo.environment["GITHUB_ACTIONS"] == "true" else {
        throw ProbeError("Display setup is only for the isolated CI runner")
    }
    try await makeRoomForFixtures()
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
            if args.count == 2 && args[1] == "verify-display" {
                try requireFixtureDisplay(); return
            }
            guard args.count >= 3, let owner = pid_t(args[1]), owner > 1, let app = NSRunningApplication(processIdentifier: owner) else {
                throw ProbeError("Expected an owned GUI PID and action")
            }
            let values = args.dropFirst(3)
            let result: Any
            switch args[2] {
            case "tree": result = try tree(ownedWindow(owner))
            case "windows": result = ownedWindows(owner).flatMap(tree)
            case "close-window":
                let window = try ownedWindow(owner)
                guard let button: AXUIElement = attribute(window, kAXCloseButtonAttribute),
                      AXUIElementPerformAction(button, kAXPressAction as CFString) == .success else {
                    throw ProbeError("Could not close the owned desktop window")
                }
                return
            case "resize-content": try resizeContent(ownedWindow(owner), values); return
            case "focus-text", "fill-text":
                guard let name = values.first else { throw ProbeError("Missing text control name") }
                let text = values.dropFirst().first
                guard args[2] != "fill-text" || text != nil else { throw ProbeError("Missing text control value") }
                try editTextControl(ownedWindow(owner), name, value: text); return
            case "select-entry":
                guard let name = values.first else { throw ProbeError("Missing picker entry name") }
                try selectDialogEntry(owner, name); return
            case "activate":
                guard app.activate(options: []) else { throw ProbeError("Could not activate the owned app") }
                return
            case "key":
                try requireFrontmost([owner])
                try key(owner, values, throughWindowServer: true)
                return
            case "press", "press-control":
                guard let name = values.first else { throw ProbeError("Missing native control name") }
                try pressControl(owner, name, modal: args[2] == "press"); return
            case "paste":
                guard let text = values.first else { throw ProbeError("Missing clipboard fixture") }
                try requireFrontmost([owner])
                let pasteboard = NSPasteboard.general
                pasteboard.clearContents()
                guard pasteboard.setString(text, forType: .string) else { throw ProbeError("Could not set the fixture clipboard") }
                try key(owner, ["9", "c"][...], throughWindowServer: true); return
            case "clipboard": result = ["value": NSPasteboard.general.string(forType: .string) ?? ""]
            case "click": try click(owner, values, ownedWindow(owner)); return
            case "capture":
                guard let target = values.first, let bounds = rectangle(try ownedWindow(owner)) else {
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
