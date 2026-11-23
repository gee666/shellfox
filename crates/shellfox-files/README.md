# shellfox-files

The host's file service: browsing, previews, saves, chunked transfers, terminal uploads and the WSL Files worker. It has no network or UI dependency. User-facing behavior and the wire API are in `docs/files-api.md`.

## Pieces

- `FileService` holds one capability: an approved root, the local desktop account, an approved full account, or a private directory scope. Every operation resolves its path through that capability, one component at a time, without following links.
- `private` creates, verifies, reads and atomically replaces owner-only files and directories relative to a held parent handle.
- `handle` has the identity, no-follow and single-link checks that other crates share.
- `TransferRegistry` and `UploadInbox` run chunked transfers and terminal uploads on top of `FileService`.
- `EditorBackend` gives the desktop editor sessions that share one transfer registry.

## Guarantees

- Paths stay inside their root. `..`, drive-relative, UNC, device and stream names are refused, and a symlink, junction or other reparse point is never followed.
- Writes are staged in an owner-only temp file, flushed and renamed. The rename happens only if the target still has the expected revision. One process-wide lock (`MUTATIONS`) covers the check and the rename, so two writers in this process cannot both pass the same check.
- New private objects are owner-only from creation: the DACL is passed to `NtCreateFile` on Windows, and the mode to `mkdirat`/`openat` on Unix.
- Existing private objects are verified, not repaired. Only `repair_app_dir` and `private_root` ever tighten permissions, and only on the app's own data directory.

Not covered: other processes running as the same user, and power-loss durability.

## Tests

```sh
cargo test -p shellfox-files
cargo clippy -p shellfox-files --tests -- -D warnings
```

Fixtures live under the repository's `tmp/`. Windows ACL and junction tests run on Windows. The Unix paths compile for Linux and macOS, but this repository runs them only in CI.
