# Files

## Using the panel

The folder icon opens or hides Files. It starts in the selected session's working directory, or the host's home directory. Terminal file links open in the same panel, including source positions such as `#L12C3` and `:12:3`. The path entry is shared with the new-session folder picker.

Text has a wrap toggle that changes only the display. Images have rotate, flip, crop and reset, with Save last. Mobile uses separate browser and file pages, one compact toolbar, and a More menu for secondary actions.

## Where files live

File operations run on the selected Shellfox host, desktop or headless, under that host's account and file permissions. A remote viewer never resolves paths against its own filesystem. WSL paths keep their tab's distribution and are never reinterpreted as Windows paths. Nested SSH/SFTP browsing is not implemented; connect that machine as a host instead.

Previews are held in the viewer's memory, not written to Downloads. Unsaved drafts live in memory until saved or discarded; a crash can lose them. Download explicitly saves a device file. Upload reads a picked file and never modifies it.

HTML and Markdown previews get their sibling assets from the host, limited to the file's folder and an allowlist of types. HTML runs sandboxed on the viewing device with no native bridge, host commands or network access.

## Saving and conflicts

Every save carries the SHA-256 revision the editor read. The host writes a private temp file next to the target, flushes it, checks that the target still has that revision, and renames the temp file over it. A changed file fails with `FILE_CONFLICT`; nothing is retried. Create mode never overwrites an existing name. The replacement keeps the old file's owner and permissions (DACL on Windows, mode and ACL on Unix), and read-only targets are refused.

## Limits

| Operation | Limit |
| --- | --- |
| Inline text read/write | 128 KiB |
| Inline preview and asset | 2 MiB |
| Inline image save | 512 KiB |
| Editor text (chunked) | 8 MiB of UTF-8 |
| Editor image and each preview asset | 64 MiB |
| Upload/download | 256 MiB by default, up to 1 GiB (`SHELLFOX_FILE_TRANSFER_MAX_BYTES`) |
| Transfer chunk | 192 KiB |
| Open transfers | 2 per owner, 8 per host |
| Idle transfer | 5 minutes |
| Directory listing | 500 entries or 256 KiB |

## Wire API

The host serves `list`, `read`, `write`, `writeImage` and `readAsset`, plus streaming transfers:

- `beginRead({path, assetPath?})` returns metadata and a `transferId`.
- `readChunk({transferId, offset})` returns data, the next offset and EOF.
- `beginWrite({path, size, mode, revision?, content, mimeType?})` opens a staged create or a revision-checked replace.
- `writeChunk({transferId, offset, data})` appends the next chunk. Repeating the last chunk exactly is a no-op.
- `finishWrite({transferId})` validates and commits. A repeated finish returns the first result.
- `cancelTransfer({transferId})` drops the transfer and its temp file.

Transfer IDs are bound to the connection that opened them, and approval is checked again just before a write commits. Request structs reject unknown fields.

## Rust crate

`shellfox-files` implements all of this. `FileService::approved_root(path, access)` scopes requests to one directory. `local_desktop` and `approved_full_account` grant the account's own paths and are for trusted host code only. `revoke()` waits for a commit in progress and then denies everything.

`shellfox_files::private` creates, reads and replaces the host's own owner-only files. `FileService::private_root` creates an app data directory privately, and `private_scope` wraps a held private directory for leaf reads and writes.

These checks protect against other users and against path tricks in requests: traversal, links, junctions and aliased names. They do not make the account a sandbox. Another process running as the same user can still change a file between the final check and the rename.
