# External desktop launches

Explorer, the `shellfox` command and a second click on the app all start the desktop binary with a launch intent. One UI per data directory shows it.

## Arguments

`app_launch::parse_launch_args` reads the launch arguments after the global flags:

- none: show the app;
- `start|add|new|create [path]`: a new session in `path`, resolved against the caller's working directory, or in that directory;
- `--new-session --cwd ABSOLUTE`: the same, as Explorer passes it;
- `--new-window`: show the result in a new window.

Each launch gets a request ID, so a retransmitted request is handled once. Paths are checked lexically first; the host checks that the directory exists before it creates anything.

## Routing

The host routes every launch (shellfox-service `desktop_launch*.rs`). Its local API takes `Discover`, `Submit` and `Receipt`, plus the `*Attached` requests a desktop window uses to register, poll, claim and complete launches.

- The first window of a UI registers as the host's desktop (`launch_runtime.rs`) and submits its own launch.
- A later launch finds a host whose route says a desktop is registered. It submits its intent to that desktop's owner fence and exits once the receipt says `Handled` (`app_startup::handoff`). It never starts a second UI or host.
- The registered desktop claims each delivery, applies it to a window (selects or opens a session, or opens a new window) and completes it. A claim checks again that the owner, registration and target session are still current.
- A failed or uncertain launch is never replayed to another window or owner.

`shellfox-cli` submits launches the same way (`Command::DesktopLaunch`). A host without a desktop rejects them with `NoDesktop`.

The host keeps at most 64 pending launches, 32 undelivered ones and 1024 receipts.
