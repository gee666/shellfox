# Database retention

The host keeps its metadata in `shellfox-rust-v1.sqlite3` in its data directory (`<Shellfox>/rust-v1/host` for installed builds, see the [README](../README.md#where-data-lives)). It uses WAL journaling and schema version 6 (`crates/shellfox-host/src/storage.rs`). Only one host opens a data directory at a time. A host refuses a store written by a newer schema.

## What it stores

- Sessions: ID, title, folder, created, updated, settled and pinned times, and the session's environment variables.
- Tabs: ID, session, title, folder, order, creation time, terminal profile, the WSL account a WSL tab first launched as, and whether the tab is pending or archived.
- Settings, including host-wide terminal environment variables, plus device approvals, desktop window state and the request IDs that stop a repeated `session create` from making a second session.

Environment variables can hold secrets. They stay until you edit them or delete the session.

The database has no commands, terminal output, scrollback or screen contents. Those live in the host's memory and end with it. Shells may still write their own history files.

## What gets removed

Closing a tab removes its row once the host has confirmed that the tab's processes are gone. A tab whose shell exits on its own is removed the same way. A tab whose shell failed to start keeps its error, and an automation tab keeps its result. A session whose last tab is removed stays. Deleting a session needs confirmation (`--yes` in the CLI). The host closes its terminals and removes the session and its tabs only after every close is confirmed. If a close fails, the session stays, and terminals that haven't closed keep running.

Settling only records a time, and unsettling clears it. Neither stops terminals or removes anything.

A restart doesn't restore shells. Saved sessions come back with the tabs that were still open, and selecting one opens a fresh shell in its first tab, or in a new tab if it has none. A crash or a stop isn't a shell exit, so it removes no tabs.

Deleting removes database rows. It isn't secure erasure. SQLite free pages, the WAL file and backups can still hold old bytes.

## Automatic settling and deletion

Settings has **Settle after (hours)** and **Delete settled after (hours)**, 24 and 720 (30 days) by default. Both accept 1 to 87600 hours. Zero or an empty field is refused; a value missing from saved settings uses the default.

A session settles after the first interval without activity. Its terminals keep running. After the second interval, counted from the later of settling and the last activity, the host closes the session's terminals and deletes it, as an explicit delete does. If a close can't be confirmed, the session stays and the host tries again a minute later. Pinned sessions are never settled or deleted automatically, including ones settled by hand.

Activity is typed input, paste, mouse clicks and scrolling, terminal output, a change in a shell's processes, a tab exiting, selecting the session, pinning or unpinning it, and renaming it or changing its folder. Focus changes and mouse motion without buttons don't count.

Before settling or deleting a session with running tabs, the host inspects each tab's processes again, hidden sessions included, and activity found then postpones the step. A WSL tab's evidence is its local `wsl.exe`, as in 0.9; processes inside the distribution aren't seen. Without fresh evidence for every running tab, the step waits a minute and tries again.

The host works out when the next session is due and wakes at that time. The first check runs a minute after the host starts. Saved times survive restarts: a session that went idle while the host was off settles a minute after start, and its deletion interval starts then. Desktop and headless hosts behave the same. Clients see the result as they see a manual settle or delete.

## Data from 0.9

The Electron database `manager.sqlite3` sits one level up, in `<Shellfox>`. The first Rust start imports settings and sessions, with their activity, settled and pinned times, from a private copy of it. Tabs aren't imported: opening an imported session starts a new shell. The host deletes the original 30 days later. See [updating from Electron](self-update.md#updating-from-electron-0915).
