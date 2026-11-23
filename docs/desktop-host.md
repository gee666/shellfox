# Desktop host process

The desktop UI (`shellfox`) runs no shells. They live in a separate host process, `shellfox-cli --desktop-host`, and the UI is a client of its local API, so a UI crash leaves every shell running.

## Startup

`app_startup::prepare` runs before GPUI:

- If a host answers on the data directory and already has a registered desktop window, the launch is handed to it (see [desktop-external-launch.md](desktop-external-launch.md)) and this process exits.
- If a host answers without a desktop, the UI attaches to it. A desktop-started host of another build is replaced when it is idle and refused ("A different Shellfox version is still running") when it has shells or a window. A `host run` host is attached as is.
- Otherwise the UI starts a host in the background and initializes meanwhile. Windows wait for the host to answer. Of two concurrent launches, one host wins the directory lock and the other exits.

The host is spawned from `shellfox-cli` in `bin/` or beside the desktop binary. On Windows it gets a hidden console of its own and breaks away from the UI's job. On Linux it runs in its own systemd user scope with `OOMPolicy=continue`, falling back to a plain `setsid` child. The host prints one line, its readiness marker or its startup error.

A desktop-started host is recorded in `desktop-host-owner` in the desktop directory, so a UI that reattaches after a crash still owns it.

## Quit and close

`AppHost` (app_host.rs) keeps one client per window. Closing a window detaches its client. Closing the last window, or Quit, stops a desktop-started host. It asks first when the host has live shells, and waits for the host process to exit. Unsaved files and pending operations block a close. Closing the desktop never stops a `host run` host.

Before the UI stops the host it writes a quit byte to the host's stdin. If the pipe closes without it, the UI died: the host starts it again once with `--owned-host`, unless the host is stopping. Later UIs attach on their own.

If the host ends without the UI asking (host_lost.rs), the UI quits when its own host was stopped, offers Restart when it died, and waits to reconnect to a `host run` host.

On macOS, Dock Quit, Cmd+Q sent from outside the app and AppleScript quit go through the same confirmation, as Electron's `before-quit` did. GPUI 0.2.2's app delegate cannot refuse a terminate, so macos_terminate.rs adds `applicationShouldTerminate:` to it. That method cancels any terminate Shellfox or a logout did not ask for, and runs the menu's Quit instead.

The end-of-app work runs once on every OS (app_exit.rs): stop or leave the host, then install a downloaded update once the host is gone. macOS exits inside GPUI's quit hook, so the hook runs it.

## Logout and shutdown

The OS ending the UI is an approved Quit with no prompt, as in 0.9.15. The host stops and its shells end:

- Windows: `WM_ENDSESSION` closes the windows (accessibility/windows_end_session.rs). The host handles console close, logoff, shutdown and break as a stop.
- Linux: SIGTERM or SIGHUP to the UI, or the display going away. The host stops on SIGTERM or SIGHUP. If its UI dies while logind is closing the session or the Wayland/X11 display is gone, the host stops instead of relaunching the UI (shellfox-service desktop_session.rs).
- macOS: the power-off notification before logout, restart and shutdown, and SIGTERM or SIGHUP (system_end.rs). The quit that follows is let through, and the host stops in GPUI's quit hook.

An unsaved file cannot hold up a logout. After 10 seconds the UI quits and stops the host anyway.
