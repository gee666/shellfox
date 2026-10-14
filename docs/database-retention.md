# Database retention

Shellfox stores `manager.sqlite3` in Electron's user-data directory. It uses SQLite WAL journaling and schema version 5.

## What stays

- Live sessions retain their ID, title, working directory, adapter, shell ID/executable, creation/update timestamps, pin timestamp, configured environment variables and session error.
- Open or unresolved tabs retain their ID, session ID, title, working directory, order, creation timestamp, lifecycle, generation/operation ID and error. Embedded metadata contains the profile ID, user-title override and exit-code field. Legacy records may contain shell registrations and window/member ownership identities until retired.
- Launch operations retain request IDs, kind, state, timestamps and errors while their tab exists. Request deduplication ends when the tab and its operations are removed.
- Archived sessions retain session metadata, including configured environment values, errors and pins. They have no tabs, launch operations, process registrations or window bindings. Restore keeps the session ID and settings, not old tab names, order, profiles or terminal contents. Opening the restored session creates a new tab.
- App settings, process-matching rules, integration preferences and manager-window geometry remain independent of session deletion.

Configured environment values can contain secrets. They remain in an archived session until the session is deleted or its environment is edited. Shellfox does not persist inherited shell environments wholesale.

## What is removed

A confirmed tab exit removes the tab row, all its launch/retry operations and request IDs, terminal metadata and legacy member metadata. The same applies to failed or cancelled launches that own no live shell. Session-level launch errors remain on the session for reporting.

Archiving closes every owned terminal first. It refuses to archive if startup or process cleanup remains unconfirmed. Only after successful closure does it save the archive timestamp and remove every tab, operation and legacy session-window binding. Closing some tabs before a later closure fails does not undo their real exits; the session stays unarchived and still-running tabs remain owned.

Deleting an archived session removes the session and all dependent rows and keyed preferences in one transaction. CLI deletion can also close and delete an active session after explicit confirmation. Missing sessions leave other sessions untouched.

At restart, old embedded handles cannot be restored from saved PIDs. Their tabs and dependent records are removed without launching a shell. Empty embedded sessions stay live and can be reopened. Retired external sessions become read-only session history; Shellfox removes their tab/root records without observing or killing external processes.

## Existing databases

The version 5 migration transaction removes old closed tabs, all archived-session tabs and operations, orphan launch/terminal records and stale `tab-member:` / `session-window:` preferences. It preserves session metadata, active open-tab data, settings and unrelated preferences. A failed migration rolls back its deletions and version change; newer schema versions are refused.

There is no deleted-session table or soft-delete flag in the existing schema. Previously deleted sessions were already removed; the migration cleans obsolete dependent records where present.

## Not command history

The database contains no shell commands, PTY output, scrollback, serialized terminal screens or process-statistics history. Terminal output and screen restoration buffers live in memory. Shells can still write their own history files outside this database.

Deletion here means removal from the application's database records. It is not a forensic secure-erasure guarantee: SQLite free pages, WAL files, filesystem snapshots and existing upgrade backups can contain older bytes. Shellfox does not erase backups or unrelated files as part of retention cleanup.
