import AppKit
import CoreGraphics

@MainActor
private func fitsFixture() -> Bool {
    guard let visible = NSScreen.main?.visibleFrame else { return false }
    // Room for a 1200×760 content area, native chrome and the window margin.
    return visible.width >= 1240 && visible.height >= 820
}

@MainActor
func requireFixtureDisplay() throws {
    guard fitsFixture() else {
        throw ProbeError("The Aqua session does not have room for the required 1200×760 content viewport.")
    }
}

@MainActor
func makeRoomForFixtures() async throws {
    if fitsFixture() { return }
    let display = CGMainDisplayID()
    let options = [kCGDisplayShowDuplicateLowResolutionModes as String: true] as CFDictionary
    let modes = (CGDisplayCopyAllDisplayModes(display, options) as? [CGDisplayMode]) ?? []
    let candidates = modes.filter { $0.width >= 1280 && $0.height >= 900 }
        .sorted { $0.width * $0.height < $1.width * $1.height }
    for mode in candidates.prefix(12) {
        var transaction: CGDisplayConfigRef?
        guard CGBeginDisplayConfiguration(&transaction) == .success, let transaction else { continue }
        if CGConfigureDisplayWithDisplayMode(transaction, display, mode, nil) != .success {
            CGCancelDisplayConfiguration(transaction)
            continue
        }
        // The preflight process exits before the actual desktop apps are started.
        guard CGCompleteDisplayConfiguration(transaction, .forSession) == .success else { continue }
        try await Task.sleep(for: .milliseconds(400))
        if fitsFixture() {
            print("P4 Aqua display for this session: \(mode.width)×\(mode.height) points")
            return
        }
    }
    let available = Set(modes.map { "\($0.width)×\($0.height)" }).sorted().joined(separator: ", ")
    throw ProbeError("The runner cannot fit the required 1200×760 P4 content viewport. Available modes: \(available). Use an isolated graphical Mac with a larger display.")
}
