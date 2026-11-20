# Mobile terminal

Android and iOS render the terminal with the native Ghostty view. TypeScript
owns the screen and controls; the native Rust client owns terminal attachment,
recovery, input and resize. See [the mobile client README](../rust-client/README.md).

Terminal attachments use the native document picker and bounded uploads. The
WebView dependency stays for file previews and the image editor, not terminal I/O.

Run `pnpm test:unit mobile/src` for the presentation tests. The Android
`nativeFixture` instrumentation and the iOS XCTest suites exercise native
behavior; neither needs a real user's device or tailnet.
