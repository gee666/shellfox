# Database retention

The host keeps its metadata in `shellfox-rust-v1.sqlite3` in its data directory (`<Shellfox>/rust-v1/host` for installed builds, see the [README](../README.md#where-data-lives)). It uses WAL journaling and schema version 6 (`crates/shellfox-host/src/storage.rs`). Only one host opens a data directory at a time. A host refuses a store written by a newer schema.

## What it stores

- Sessions: ID, title, folder, created, updated, settled and pinned times, and the session's environment variables.
- Tabs: ID, session, title, folder, order, creation time, terminal profile, the WSL account a WSL tab first launched as, and whether the tab is pending or archived.
- Settings, including host-wide terminal environment variables, plus device approvals, desktop window state and the request IDs that stop a repeated `session create` from making a second session.

Environment variables can hold secrets. They stay until you edit them or delete the session.

The database has no commands, terminal output, scrollback or screen contents. Those live in the host's memory and end with it. Shells may still write their own history files.

## What gets removed

Closing a tab removes its row once the host has confirmed that the tab's processes are gone. Deleting a session needs confirmation (`--yes` in the CLI). The host closes its terminals and removes the session and its tabs only after every close is confirmed. If a close fails, the session stays, and terminals that haven't closed keep running.

Settling only records a time, and unsettling clears it. Neither stops terminals or removes anything.

A restart doesn't restore shells. Saved sessions come back, and selecting one opens a fresh shell.

Settings has **Settle after (hours)** and **Delete settled after (hours)**, 24 and 720 by default. The host saves them but doesn't settle or delete sessions on its own yet.

Deleting removes database rows. It isn't secure erasure. SQLite free pages, the WAL file and backups can still hold old bytes.

## Data from 0.9

The Electron database `manager.sqlite3` sits one level up, in `<Shellfox>`. The first Rust start imports settings and sessions from a private copy of it, and the host deletes the original 30 days later. See [updating from Electron](self-update.md#updating-from-electron-0915).
