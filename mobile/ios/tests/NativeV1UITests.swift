import XCTest

final class NativeV1UITests: XCTestCase {
  private let bundleID = "com.shellfox.mobile.iosnativev1fixture"
  private let control = "http://127.0.0.1:42761"

  override func setUp() {
    super.setUp()
    continueAfterFailure = false
    XCUIDevice.shared.orientation = .portrait
  }

  override func tearDown() {
    XCUIDevice.shared.orientation = .portrait
    super.tearDown()
  }

  func testNativeV1OnOwnedRustHost() throws {
    let app = XCUIApplication(bundleIdentifier: bundleID)
    app.launchArguments = ["--shellfox-native-v1", "--shellfox-control-url", control]
    app.launchEnvironment["TS_DEBUG_ALWAYS_USE_DERP"] = "true"
    // xcodebuild can replace or terminate the previous simctl launch.
    app.launch()

    let connect = app.buttons["Connect"].firstMatch
    XCTAssertTrue(connect.waitForExistence(timeout: 60), app.debugDescription)
    connect.tap()
    XCTAssertTrue(app.buttons["Cancel request"].waitForExistence(timeout: 30), app.debugDescription)
    screenshot("01-awaiting-approval")

    let session = app.buttons["Open iOS native CI"]
    XCTAssertTrue(session.waitForExistence(timeout: 60), app.debugDescription)
    session.tap()
    // The UIKit terminal provides the actual painted text as its accessibility value.
    // The web terminal does not expose this native element.
    let terminal = app.descendants(matching: .any).matching(NSPredicate(format: "label == %@", "Terminal")).firstMatch
    XCTAssertTrue(terminal.waitForExistence(timeout: 30), app.debugDescription)
    focusKeyboard(app, terminal: terminal)
    app.typeText("SFX_CI_TOKEN=retained; printf '%s%s:' SF_ IOS_PORTRAIT; stty size\n")
    let portrait = try terminalGrid(terminal, marker: "SF_IOS_PORTRAIT")
    screenshot("02-native-echo-portrait")

    XCUIDevice.shared.orientation = .landscapeLeft
    wait("simulator rotated") { app.frame.width > app.frame.height }
    focusKeyboard(app, terminal: terminal)
    app.typeText("printf '%s%s:' SF_ IOS_LANDSCAPE; stty size\n")
    let landscape = try terminalGrid(terminal, marker: "SF_IOS_LANDSCAPE")
    XCTAssertGreaterThan(landscape.columns, portrait.columns)
    screenshot("03-native-resized-landscape")

    let connection = app.descendants(matching: .any)["connection-status"].firstMatch
    wait("connected before the outage") { connection.exists && connection.label == "Connection connected" }
    app.typeText("printf ready > .ios-outage-ready\n")
    wait("the attached simulator observed real host loss") {
      connection.exists && ["Connection disconnected", "Connection reconnecting..."].contains(connection.label)
    }
    screenshot("04-native-host-offline")
    wait("the same session reconnects to the restored host", timeout: 75) {
      connection.exists && connection.label == "Connection connected" && terminal.exists
    }
    focusKeyboard(app, terminal: terminal)
    app.typeText("printf '%s%s:%s\\n' SF_ IOS_RECONNECTED \"$SFX_CI_TOKEN\"\n")
    wait("the same PTY after network reconnect") { self.value(terminal).contains("SF_IOS_RECONNECTED:retained") }
    screenshot("05-native-reconnected")

    XCUIDevice.shared.press(.home)
    wait("app entered background") {
      app.state == .runningBackground || app.state == .runningBackgroundSuspended
    }
    app.activate()
    XCTAssertTrue(terminal.waitForExistence(timeout: 30), app.debugDescription)
    focusKeyboard(app, terminal: terminal)
    app.typeText("printf '%s%s:%s\\n' SF_ IOS_RESUMED \"$SFX_CI_TOKEN\"\n")
    wait("retained PTY after foreground reattach") { self.value(terminal).contains("SF_IOS_RESUMED:retained") }
    screenshot("06-native-foreground-reattached")

    XCUIDevice.shared.orientation = .portrait
    let files = app.buttons["Files"]
    XCTAssertTrue(files.waitForExistence(timeout: 20), app.debugDescription)
    files.tap()
    let file = app.buttons["Open file fixture.txt"]
    XCTAssertTrue(file.waitForExistence(timeout: 30), app.debugDescription)
    file.tap()
    let edit = app.buttons["Edit source"]
    XCTAssertTrue(edit.waitForExistence(timeout: 20), app.debugDescription)
    edit.tap()
    let source = app.textViews["file-source"].firstMatch
    XCTAssertTrue(source.waitForExistence(timeout: 20), app.debugDescription)
    XCTAssertTrue(value(source).contains("initial fixture text"))
    source.tap()
    source.typeText("saved-from-ios")
    let save = app.buttons["files-save"]
    wait("file is ready to save") { save.exists && save.isEnabled }
    save.tap()
    wait("file saved") { save.exists && !save.isEnabled && save.label != "Saving" }
    screenshot("07-native-file-saved")
  }

  private func focusKeyboard(_ app: XCUIApplication, terminal: XCUIElement) {
    if !app.keyboards.firstMatch.exists {
      let keyboard = app.buttons["Keyboard"]
      if keyboard.exists { keyboard.tap() } else { terminal.tap() }
    }
    XCTAssertTrue(app.keyboards.firstMatch.waitForExistence(timeout: 15), app.debugDescription)
  }

  private func terminalGrid(_ terminal: XCUIElement, marker: String) throws -> (rows: Int, columns: Int) {
    let pattern = try NSRegularExpression(pattern: "\(marker):\\s*([0-9]+)\\s+([0-9]+)")
    func readGrid() -> (Int, Int)? {
      let screen = value(terminal)
      guard let m = pattern.firstMatch(in: screen, range: NSRange(screen.startIndex..., in: screen)),
            let rows = Range(m.range(at: 1), in: screen), let cols = Range(m.range(at: 2), in: screen),
            let rowCount = Int(screen[rows]), let colCount = Int(screen[cols]) else { return nil }
      return (rowCount, colCount)
    }
    wait("native output and host PTY dimensions for \(marker)") { readGrid() != nil }
    return try XCTUnwrap(readGrid(), value(terminal))
  }

  private func value(_ element: XCUIElement) -> String {
    element.value as? String ?? ""
  }

  private func wait(_ description: String, timeout: TimeInterval = 30, until predicate: @escaping () -> Bool) {
    let matches = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in predicate() }, object: nil)
    XCTAssertEqual(XCTWaiter.wait(for: [matches], timeout: timeout), .completed, description)
  }

  private func screenshot(_ name: String) {
    let attachment = XCTAttachment(screenshot: XCUIScreen.main.screenshot())
    attachment.name = name
    attachment.lifetime = .keepAlways
    add(attachment)
  }
}
