# Database retention

Shellfox stores `manager.sqlite3` in Electron's user-data directory. It uses SQLite WAL journaling and schema version 5.

## What stays

- Live sessions retain their ID, title, working directory, adapter, shell ID/executable, creation/update timestamps, pin timestamp, configured environment variables and session error.
- Open or unresolved tabs retain their ID, session ID, title, working directory, order, creation timestamp, lifecycle, generation/operation ID and error. Embedded metadata contains the profile ID, user-title override and exit-code field. Legacy records may contain shell registrations and window/member ownership identities until retired.
- Launch operations retain request IDs, kind, state, timestamps and errors while their tab exists. Request deduplication ends when the tab and its operations are removed.
- Settled sessions retain the same session and tab data as other sessions. Settling only records a timestamp; unsettling clears it. Neither action closes terminals, stops agents or removes launch operations.
- App settings, process-matching rules, integration preferences and manager-window geometry remain independent of session deletion.

Configured environment values can contain secrets. They remain in a settled session until the session is deleted or its environment is edited. Shellfox does not persist inherited shell environments wholesale.

## What is removed

A confirmed tab exit removes the tab row, all its launch/retry operations and request IDs, terminal metadata and legacy member metadata. The same applies to failed or cancelled launches that own no live shell. Session-level launch errors remain on the session for reporting.

Settling is non-destructive. The `settle` and `unsettle` commands change status without confirmation and leave live terminals and agents alone.

Deleting a session closes its owned terminals and removes the session, dependent rows and keyed preferences after confirmation. If terminal closure cannot be confirmed, deletion fails. Closing some tabs before a later closure fails does not undo their exits; still-running tabs remain owned. Missing sessions leave other sessions untouched.

At restart, old embedded handles cannot be restored from saved PIDs. Their tabs and dependent records are removed without launching a shell. Empty embedded sessions stay live and can be reopened. Retired external sessions keep their saved metadata; Shellfox removes their tab/root records without observing or killing external processes. Opening one starts a new embedded shell.

## Automatic lifecycle

`sessionSettleHours` defaults to 24 hours. `sessionDeleteHours` defaults to 720 hours, or 30 days. Both are configurable. A minute sweep marks inactive sessions settled after the first interval. That starts a fresh deletion grace period, including after a long shutdown. After the second interval without activity, the sweep closes owned terminals and deletes unpinned settled sessions. Pinned sessions are exempt from automatic deletion, not from settling or explicit deletion.

Session selection, terminal input/output and process activity update the session's activity time. The deletion interval starts from whichever is later, the settled timestamp or the latest activity. Automatic cleanup uses the same ownership checks as explicit deletion and retries on later sweeps if closure or storage fails.

## Existing databases

The version 5 migration transaction removes old closed or orphaned tabs, orphan launch/terminal records and stale `tab-member:` / `session-window:` preferences. It preserves session metadata, open-tab data, settings and unrelated preferences regardless of settled status. A failed migration rolls back its deletions and version change; newer schema versions are refused.

There is no deleted-session table or soft-delete flag in the existing schema. Previously deleted sessions were already removed; the migration cleans obsolete dependent records where present.

## Not command history

The database contains no shell commands, PTY output, scrollback, serialized terminal screens or process-statistics history. Terminal output and screen restoration buffers live in memory. Shells can still write their own history files outside this database.

Deletion here means removal from the application's database records. It is not a forensic secure-erasure guarantee: SQLite free pages, WAL files, filesystem snapshots and existing upgrade backups can contain older bytes. Shellfox does not erase backups or unrelated files as part of retention cleanup.
