# Mobile terminal

`TerminalView` renders xterm 6 in a local WebView. It needs a `SafeAreaProvider` above it and a parent with a bounded height, normally `flex: 1`.

```tsx
<TerminalView
  api={client.terminalApi}
  subscribeTerminal={client.subscribeTerminal}
  tabId={tab.id}
  fontSize={14}
  connectionEpoch={clientState.connectionId ?? 'disconnected'}
  onError={error => showError(error.message)}
/>
```

Mount while the client is connected. Keep `api` and `subscribeTerminal` identities stable. A changed `connectionEpoch` detaches and reattaches without restarting the shell. Only mount one view per client/tab. Pass the stable `terminalApi` object from `RemoteClient`, not unbound class methods.

The bridge validates messages, replays at the host's grid, waits for xterm parsing and a paint before acknowledging output, and recovers sequence gaps with a fresh bounded replay. It renews successful control leases every ten seconds with a resize, never artificial terminal input. Failed writes are not repeated.

Tap the terminal or use Keyboard to show the native keyboard. Ctrl stays active until tapped again. Use Select and drag over terminal cells, or touch and hold, then Copy to open selectable native text. The native text menu copies the selection. Paste opens a native text field with the OS paste menu; Send routes text through xterm's bracketed-paste handling. Ordinary touch scrolling still uses xterm's scrollback.

## Build and tests

`terminal-html.ts` is generated and ignored. The mobile start, Android, iOS and typecheck scripts generate it explicitly, without relying on package-manager pre-hooks. Distribution builds must run the generator before Metro as well.

```sh
node scripts/mobile-terminal.mjs
node scripts/mobile-terminal.mjs --check
pnpm test:unit mobile/src/terminal
pnpm exec playwright install chromium
node --test mobile/src/terminal/terminal-page.test.mjs
pnpm typecheck
```

The generator bundles xterm, addon-fit, CSS and the page bridge into one TypeScript HTML export. No external scripts, fonts or styles are loaded. A script hash in the page CSP blocks unapproved scripts, and the WebView rejects navigation outside its initial `about:blank` document. Host output travels only as JSON messages, never as JavaScript source.

## Native Android regression

The isolated `terminalTest` variant uses `com.shellfox.mobile.terminaltest`, not the normal app package. Its explicit entry mounts the production `TerminalView` with a test-only echo API. It never starts Tailscale or changes authentication.

Generate the terminal page, then build from `mobile/android` with `ENTRY_FILE=e2e/terminal-entry.js` and `:app:assembleTerminalTest -PreactNativeArchitectures=x86_64`. In PowerShell, set `$env:ENTRY_FILE` before `./gradlew.bat` and remove it afterward. In bash, prefix `./gradlew` with the environment assignment. The output is `mobile/android/app/build/outputs/apk/terminalTest/app-terminalTest.apk`.

From the repository root:

```sh
node scripts/mobile-terminal-native-e2e.mjs --serial emulator-5554 --apk mobile/android/app/build/outputs/apk/terminalTest/app-terminalTest.apk
```

The runner checks the APK manifest before installing and refuses non-emulator serials. It verifies native WebView replay, Unicode/ANSI output, real keyboard input and rendered echo, first-use Keyboard show/hide, key-row bounds above the IME, grid resizing and remount. It restores the emulator's hardware-keyboard setting and stops only the isolated test app. Screenshots and crash logs go under `tmp/mobile-terminal-native-e2e/`.

Android Fabric requires `dataDetectorTypes` to be an array. A scalar accepted by the public WebView type reaches a generated `ReadableArray` cast on Android and crashes while mounting.

The standalone Playwright test launches real Chromium with a fake `ReactNativeWebView` bridge. It checks replay rendering, keyboard and Ctrl input, Unicode text entry, bracketed paste, resize, touch selection, stale epochs, Android message dispatch and blocked networking. Native Android/iOS keyboard composition, selection menus and safe-area behavior still need device testing.
