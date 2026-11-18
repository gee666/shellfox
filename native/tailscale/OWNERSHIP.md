# Bound host ownership

The secure Rust startup carrier uses `--ownership ABSOLUTE_ROOT EXPECTED_ID`. It never recaptures the expected identity from a path after Service has validated a retained root. The caller owns a clone of that same original directory descriptor throughout preparation and acquisition.

The strict representation is:

- `root-v1:linux:<device-u64-16hex>:<inode-u64-16hex>`
- `root-v1:darwin:<native-dev_t-i32-bits-8hex>:<inode-u64-16hex>`
- `root-v1:windows:<FILE_ID_INFO-volume-u64-16hex>:<FILE_ID_128-bytes-32hex>`

Hex is fixed-width lowercase. Darwin device conversion preserves the exact native signed32 bit pattern. Windows uses `GetFileInformationByHandleEx(FileIdInfo)`, not the legacy64-bit file index. Unsupported providers and empty Windows IDs refuse, without a truncating fallback. The Rust and Go encoders use these same fields and widths.

The helper walks every original path component with no-follow directory opens. Windows keeps non-delete-sharing ancestor handles until lock setup under the pinned path is finished. Unix lookup below the verified root uses its retained directory descriptor. The final opened root identity must equal EXPECTED_ID before any private directory/file creation. An ancestor swap, symlink or mismatched root is a rejection, never another directory to repair.

Existing roots and `.host-lock/host.lock` are verification-only before the occupied decision. No chmod, DACL reset, truncation or lock-file replacement occurs. New lock objects use exclusive creation/private security at creation. Losing an exclusive-create race opens and verifies the existing object instead of adopting it as newly created. Once the original lock exists, only the actual acquired owner may mutate application layout or metadata.

## Private ACL policy

Linux rejects extended access/default ACLs on held descriptors. Darwin asks `fgetattrlist` for returned-attribute and extended-security evidence on the held descriptor. A missing, truncated, oversized or unsupported security reply refuses. Any ACE, including inherited or benign deny-only entries, refuses. Chmod is not treated as ACL removal.

This is intentionally conservative. The no-follow ancestor walk makes no privacy claim about common-home ancestors. Benign ACLs on a final app-owned object are still refused; native common-home/inheritance behavior has not been accepted on macOS. Do not describe uid/mode checks as verified macOS privacy or silently clear existing ACLs. New-object ACL initialization requires its own exclusive-creation receipt. Windows base-root explicit-SD relative creation remains Service's responsibility.

## Pre-acquisition cleanup contract

Transport's single custody path is:

1. `OfflinePreparation::new(helper)` before any anchor helper, missing-root creation await or handshake. Append `retain_original_directories(Vec<File>)` with clones of ALL original inspected ancestor handles before native work; append original newly-created handles before binding. This is bounded, append-only custody under the same admission/registration/seal lock. Every already-spawned controller holds the SAME shared guard, so later appends do not replace its earlier proof.
2. Borrow `preparation.permission_verifier()`. Every original helper is registered to that same ledger/monitor before Tokio pipe conversion or its first await. Scoped protection of existing objects is denied until actual ownership.
3. Build `BoundOwnershipRoot::from_held(path, SAME_FILE_CLONE)` from Service's held descriptor. No expected identity comes from path recapture.
4. Consume `bind_root`, receiving an `OwnershipAttempt`, or `BindRootFailure` retaining the original preparation and error. This transition does no IO and recreates no resources.
5. Borrow `attempt.acquire()`. `Occupied` contains sealed no-owner proof only after the original contender and every preparation child have been waited. An error leaves the attempt usable for cleanup.
6. `into_acquired` transfers the same root, preparation and original controller once. Failure retains the attempt and error. The acquired owner's verifier is borrowed from that same custody.

`OwnershipAttempt::root_lifetime()` and `HostOwnership::root_lifetime()` return an opaque clonable `RootLifetimeGuard`. It retains original directories only; it exposes no path or authority claim. The object owning Host must keep that guard through SQLite close and any deferred retry custody. A thread-local closure alone is insufficient when Service may escape it. This is not `Host::open_retained`, a rooted VFS or Unix confinement; that integration remains a separate reviewed gate.

`shutdown_confirmed` freezes admissions under the same lock used for spawn/registration, requests original cleanup, then waits. No-child and positively waited originals may produce `NoOwnerProof`. Wait errors, timeout, lost health, stdout EOF and phase labels never produce it. Cancellation retains the registered original controllers. Existing compatibility `acquire` also opens a no-follow retained root and sends its expected identity; it must not replace Service's already-held-root API.

The CLI contract changed. Old staged helper/mobile artifacts remain immutable. Matching helper rebuild, pinning and native acceptance require separate authorization before startup admission. This remediation was checked with static Rust compilation and pure identity/cleanup/ACL parsing tests only. No Go helper, Service test binary, GUI, network, production root or account was executed.
