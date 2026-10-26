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

The standalone Playwright test launches real Chromium with a fake `ReactNativeWebView` bridge. It checks replay rendering, keyboard and Ctrl input, Unicode text entry, bracketed paste, resize, touch selection, stale epochs, Android message dispatch and blocked networking. Native Android/iOS keyboard composition, selection menus and safe-area behavior still need device testing.
