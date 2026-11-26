# Desktop host process

The desktop UI (`shellfox`) runs no shells. They live in a separate host process, `shellfox-cli --desktop-host`, and the UI is a client of its local API, so a UI crash leaves every shell running.

## Startup

`app_startup::prepare` runs before GPUI. Like 0.9's single-instance lock, one launch at a time gets past the host lookup: a launch holds `<data>.launch.lock` until its window registers with the host, and a second launch waits up to 60 s for it, then hands itself to that window. The process whose window registered then holds `<data>.window.lock` while its host serves; a launch that sees it held retries a host lookup that misses for up to 10 s instead of starting a second host.

- If a host answers on the data directory and already has a registered desktop window, the launch is handed to it (see [desktop-external-launch.md](desktop-external-launch.md)) and this process exits.
- If a host answers without a desktop, the UI attaches to it. A desktop-started host of another build is replaced when it is idle. When it has shells or a window, the UI asks first ("Shellfox was updated. N terminals are still running in the previous version." with Stop them and continue, or Quit), as this build can't show them. The dialog is a message box on Windows, zenity, kdialog or xmessage on Linux, and osascript on macOS. A `host run` host is attached as is.
- Otherwise the UI starts a host in the background and initializes meanwhile. Windows wait for the host to answer. If the started host fails, as one does that loses the directory to another host, the UI uses whichever host answers within 3 s, and reports the failure only if none does.

The host is spawned from `shellfox-cli` in `bin/` or beside the desktop binary. On Windows it gets a hidden console of its own and breaks away from the UI's job. On Linux it runs in its own systemd user scope with `OOMPolicy=continue`, falling back to a plain `setsid` child. The host prints one line, its readiness marker or its startup error.

A desktop-started host is recorded in `desktop-host-owner` in the desktop directory, so a UI that reattaches after a crash still owns it.

## Quit and close

`AppHost` (app_host.rs) keeps one client per window. Closing a window detaches its client. Closing the last window, or Quit, stops a desktop-started host. It asks first when the host has live shells, and waits for the host process to exit. Unsaved files and pending operations block a close. Closing the desktop never stops a `host run` host.

The UI writes a quit byte to the host's stdin before it stops the host and on any other deliberate exit, such as a reported startup error. After a quit an idle host stops and a busy one keeps its shells for the next UI. If the pipe closes without the byte, the UI died (a panic, a signal, a lost display): the host starts it again with `--owned-host`, up to 3 times in 5 minutes, unless the host is stopping. After that an idle host stops, and a host with shells shows an OS notification (notify-send or D-Bus on Linux, osascript on macOS, a message box on Windows): "Shellfox's window keeps closing. N terminals are still running. Run shellfox to see them." Later UIs attach on their own.

If the host ends without the UI asking (host_lost.rs), the UI quits when its own host was stopped, offers Restart when it died, and waits to reconnect to a `host run` host.

On macOS, Dock Quit, Cmd+Q sent from outside the app and AppleScript quit go through the same confirmation, as Electron's `before-quit` did. GPUI 0.2.2's app delegate cannot refuse a terminate, so macos_terminate.rs adds `applicationShouldTerminate:` to it. That method cancels any terminate Shellfox or a logout did not ask for, and runs the menu's Quit instead.

The end-of-app work runs once on every OS (app_exit.rs): stop or leave the host, then install a downloaded update once the host is gone. macOS exits inside GPUI's quit hook, so the hook runs it.

## Logout and shutdown

The OS ending the UI is an approved Quit with no prompt, as in 0.9.15. The host stops and its shells end:

- Windows: `WM_ENDSESSION` closes the windows (accessibility/windows_end_session.rs). The host handles console close, logoff, shutdown and break as a stop.
- Linux: SIGTERM or SIGHUP to the UI, or the display going away. The host stops on SIGTERM or SIGHUP. If its UI dies while logind is closing the session or the Wayland/X11 display is gone, the host stops instead of relaunching the UI (shellfox-service desktop_session.rs).
- macOS: the power-off notification before logout, restart and shutdown, and SIGTERM or SIGHUP (system_end.rs). The quit that follows is let through, and the host stops in GPUI's quit hook.

An unsaved file cannot hold up a logout. After 10 seconds the UI quits and stops the host anyway.

## Package removal and downgrade

The Linux deb's prerm stops this package's desktop and `host run` hosts, and so their shells, on removal or on a move back to Electron (below 0.10), which can't see them. Run from inside a Shellfox terminal, it stops them once dpkg exits. An upgrade to another Rust version leaves them to the new UI's question above. Squirrel kills every process under the install root when it installs, updates or uninstalls; each shell's Windows job has KILL_ON_JOB_CLOSE, so the host's death ends them.
