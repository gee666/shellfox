# Shellfox metadata host

`shellfox-host` owns the Rust experiment's session, tab and preference storage. It has no GPUI, terminal, Ghostty, Go or network dependency. `shellfox-core` contains the portable types that future mobile bindings can use without gaining local PTY access.

## Storage boundary

`Host::open(directory)` opens only `directory/shellfox-rust-v1.sqlite3`. This is a separate experiment format at schema version 1, identified by SQLite application ID `0x53465852`. It is not the Electron database and is not a replacement migration for that database. Never rename an existing Shellfox database to this filename.

The host refuses unknown application IDs, nonempty unversioned databases and newer versions. Initialization and mutations use immediate transactions. Reads use one transaction when assembling a snapshot. SQLite uses WAL, foreign keys and a five-second busy timeout. A file lock permits one host owner per data directory. Share an `Arc<Host>` between clients and threads rather than opening another owner.

Sessions and tabs have UUID identities. Tabs reference their session with a cascading foreign key. Create request IDs reference their session so retries do not create duplicates. Deletion removes requests and tabs in the same transaction and repairs selected IDs. The host validates decoded JSON, identity columns, timestamps, membership, ordering, environment and preferences before returning persisted data. Corruption causes an error, not a reset to defaults.

This first format rewrites metadata rows within each transaction. It favors straightforward atomic validation over high-volume histories. It does not store terminal output, commands, process identities, credentials or inherited environment. Explicit `set_session_env` and `set_terminal_env` calls do persist the variables the user supplies. They are not a credential store.

## Desktop API

The host re-exports the core types. All metadata mutations return an authoritative `Snapshot` with a durable revision, session list, appearance preferences and selection.

```rust,no_run
use shellfox_host::*;

let host = Host::open("tmp/my-isolated-host")?;
let snapshot = host.create_session(CreateSession {
    request_id: SessionId::new(),
    title: Some("Shellfox".into()),
    cwd: "/work/shellfox".into(),
    profile_id: ProfileId::new("login-shell")?,
})?;
let session = snapshot.sessions[0].id;
host.add_tab(session, AddTab {
    title: None,
    cwd: None,
    profile_id: ProfileId::new("pwsh")?,
})?;
host.update_preferences(PreferencesPatch {
    font_size: Some(16.0),
    ..Default::default()
})?;
# Ok::<(), shellfox_host::Error>(())
```

The remaining mutations are `rename_session`, `set_session_cwd`, `select_session`, `set_session_pinned`, `set_session_settled`, `settle_idle_session`, `rename_tab`, `set_tab_cwd`, `reorder_tabs`, `select_tab`, `close_tab`, `delete_session`, `set_session_env`, `set_terminal_env` and `save_settings`. `reorder_tabs` requires the complete permutation. Closing the final tab keeps its session. Changing a session directory changes future launch defaults, not an existing shell's working directory. Editing a live tab's launch directory is refused.

Pins preserve their original `pinnedAt` when already pinned. Pinning, unpinning and an explicit settle or unsettle count as activity and update `updatedAt`. `settle_idle_session` is the service's automatic settle and records only the settled time. The service decides when sessions settle or are deleted ([database retention](../../docs/database-retention.md#automatic-settling-and-deletion)). Tab states describe pending or archived metadata, never proof that a shell is running. Restarting the host does not resume commands or adopt saved processes.

`changed_since(revision)` returns the latest revisioned `ChangedEvent`, if newer. Clients fetch a snapshot when they miss revisions. This is a notification cursor, not an event replay log.

`request` and `settings_request` take the typed `HostRequest` and `SettingsRequest` sent over local desktop IPC, with existing Shellfox method spellings and camelCase fields. Unknown methods and fields fail to deserialize. Appearance uses `Preferences` and `PreferencesPatch`; `save_settings` atomically preserves omitted global environment.

## Terminal adapter contract

`terminal_launch(tab_id)` returns validated profile/cwd intent and explicit environment only. It does not spawn or discover a shell. Profile IDs are `pwsh`, `windows-powershell`, `cmd`, `wsl` and `login-shell`. The local adapter must check availability and enforce its own launch/isolation policy.

Register `TerminalIdentity { tab_id, generation }` with `register_terminal` before launch. Retain that registration and the actual process owner after startup failure until the runtime proves owned cleanup. Convenience `close_tab` and `delete_session` reject registered owners.

For an owned tab or session:

1. Call `prepare_close_tab` or `prepare_delete_session`. The result contains a token and every affected registered identity. New registrations for the target are blocked.
2. Ask the runtime to close each identity and await confirmed cleanup. Do not discard views or startup-error owners on failure.
3. Call `terminal_stopped` for each confirmed identity. A stale generation cannot remove a newer owner.
4. Call `commit_close(token)`. It rejects any remaining owner, changed revision or mismatched identity before deleting metadata. A failed commit keeps the token. Cancel and prepare again after unrelated metadata changes.

`cancel_close` releases the pending token but never unregisters or stops a terminal. These lifecycle methods are trusted local adapter APIs, deliberately absent from the remote metadata request enum. The host does not authenticate process-exit evidence itself; only the process-owning runtime may report it.

## Legacy import

`import_legacy(path)` reads an explicitly supplied copy of a legacy SQLite database through a read-only connection and consistent read transaction. It accepts repository schema versions 1 through 5 and imports into an untouched empty experiment store only. It never migrates or writes the source file.

Import preserves session IDs, titles, directories, UTC timestamps, settlement and pins, plus accent/background colors and the default shell profile. Legacy `bash` maps to `login-shell`. Unsupported custom profiles fail explicitly. Legacy font settings do not exist, so Rust defaults apply. The importer omits legacy tabs and ownership records because persisted registrations cannot restore process ownership. Saved session environment is imported only with `SettingsImportPolicy::IncludeEnvironment`. Operations, command lines, credentials, output and window bindings are not imported.

This is a narrow, tested import path, not complete production database compatibility. Replacing Electron storage still requires a separately reviewed migration covering the remaining settings, retention, SSH/remote state and rollback policy.

## Checks

```text
cargo test --locked -p shellfox-core -p shellfox-host --all-targets
cargo clippy --locked -p shellfox-core -p shellfox-host --all-targets -- -D warnings
```

Tests create isolated directories under the repository's root `tmp/` and remove them afterwards. Legacy fixtures cover all five schema versions, byte/schema/data preservation, committed WAL contents, hostile/corrupt schemas and atomic import rejection. Host tests cover reopen persistence, concurrent/idempotent mutations, validation rollback, preference/environment semantics, selection repair and generation-checked cleanup. Windows execution does not establish native acceptance on macOS, Linux, Android or iOS.
