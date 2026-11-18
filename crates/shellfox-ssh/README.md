# SSH host domain

This crate ports the active `src/main/ssh` profile, endpoint, key-import, known-host and PTY contracts. It belongs to the desktop/headless host. Do not add it to a remote-only Android/iOS client. Mobile RPC responses must use `RemoteProfileDto`, not `StoredProfile`, `ProfileInput` or local host capabilities.

## Host integration

```rust,no_run
use shellfox_ssh::{PrivateDirectory, ProfileStore, StoragePolicy};
use std::sync::Arc;

# fn example() -> shellfox_ssh::Result<()> {
// Choose an isolated application-owned directory with an existing trusted parent.
let policy = StoragePolicy::with_windows_helper(
    "P:/workshop/Windows_Worshop/oira666/shellfox-ghostti/tmp/tailscale/win32-x64/shellfox-tailscale.exe",
);
let private = Arc::new(PrivateDirectory::open(
    "P:/workshop/Windows_Worshop/oira666/shellfox-ghostti/tmp/my-ssh-fixture",
    policy,
)?);
let profiles = ProfileStore::new(private.clone());
let settings_rows = profiles.list()?;
let remote_rows = profiles.list_remote()?;
# let _ = (settings_rows, remote_rows);
# Ok(())
# }
```

Those paths illustrate explicit host configuration, not deployment defaults. Unix hosts use `StoragePolicy::default()`. Windows requires the existing Go helper's offline `--private-permissions ABS file|directory protect|check` command. A missing helper is an error. The host must supply its verified packaged helper; a remote request must not choose an executable or storage directory. Use ordinary absolute Windows drive/UNC paths, not a `\\?\` device path. Never point tests at installed application data.

`ProfileInput::from_json` bounds and validates input before mutation. Its password field preserves the existing three states:

| JSON | Rust | Save behavior |
| --- | --- | --- |
| absent | `PasswordUpdate::Keep` | Keep previous password, or null for a new profile |
| null | `PasswordUpdate::Clear` | Remove saved password |
| string, including empty | `PasswordUpdate::Set` | Replace saved password |

Saves reject case-insensitive duplicate names. Import matches names case-insensitively and preserves ID, password, remote folder and source. Endpoint normalization follows the current PuTTY last-`@` and ignored single-colon suffix rules. Lowercase sorting is deterministic rather than OS-locale-dependent. Reads normalize legacy endpoints in memory without rewriting the file; a subsequent matching import records that normalization.

`ProfileDto` matches local settings and excludes password/private connection options. `RemoteProfileDto` also excludes local key-file and working-directory paths. Host credential lookup is a separate `get_for_connection(id)` operation. Secrets and private proxy options redact Debug output; private persistence necessarily serializes stored secrets, so do not serialize host objects into RPC responses.

## Private persistence

Only an explicit directory is opened. The crate never discovers or copies a home SSH configuration, production profile database, or user `known_hosts` file.

- Unix directories/files require current effective UID ownership and modes 0700/0600. Executable, group/other-readable files, symlinks, special files and multiply linked files fail closed.
- Windows uses the retained Go helper to protect newly created directories/files before writing secret bytes. Existing files must already pass its ownership/protected-DACL check. The directory handle disallows delete sharing and remains open while the store is alive. There is no chmod-only or permissive test fallback.
- File operations use a pinned capability directory, no-follow opens, handle checks and bounded reads. The caller must trust the chosen parent directory. This is not a sandbox against another process running as the same user or an administrator.
- Mutations sharing a private-directory capability first queue locally, then hold a persistent OS file lock, reload current state, write a create-new private stage, sync it and atomically rename it. Local queued writes do not consume one another's cross-process lock deadline while mandatory native ACL checks run. Locks are not unlinked, avoiding stale-lock inode races. Cross-process contention still has a five-second deadline. Corrupt/unsafe stores are never replaced.
- Unix syncs the directory after rename. Windows gets atomic replacement but this implementation does not claim power-loss directory-flush durability through portable Rust APIs.

Limits are 10,000 profiles/hosts, 16 MiB profile JSON, 4 MiB trust JSON, 512 KiB profile input, 128 KiB imported private keys and 64 KiB public wire keys. Text bounds use UTF-16 length to match TypeScript. Parser errors and helper/native output never enter returned error messages.

## Keys

`keys::KeyStore::import` validates the supplied bytes, then stores the original representation under owner-only permissions. Encrypted originals remain encrypted. No decrypted key is written to disk.

Tested algorithms are RSA, Ed25519 and ECDSA nistp256/384/521. Containers include OpenSSH, PuTTY v2/v3, PKCS8, unencrypted PKCS1/SEC1 PEM, encrypted PKCS8 with modern PBKDF2 and legacy PBKDF2-HMAC-SHA1. PuTTY MACs authenticate comments and private data before decoding. Ed25519 mpint seed width and short EC scalars match the existing TypeScript implementation.

KDF work is bounded before execution. OpenSSH bcrypt allows 1..128 rounds and at most 64 salt bytes. PuTTY Argon2 allows 8..262144 KiB memory, 1..100 passes and 1..64 lanes, with memory times passes at most 1048576 KiB-passes. Salt length is 8..64 bytes. PKCS8 PBKDF2 caps iterations at 1,000,000. Scrypt caps memory at 256 MiB and cost times block size times parallelism at 8388608. AES schedules, derived buffers, decoded private buffers and owned integer-conversion scratch use the libraries' zeroization support. This is not a guarantee that every library/allocator transient copy is erased.

RSA import uses ring to check/sign the imported components, not the vulnerable RustCrypto `rsa` private-operation implementation. The current ring route accepts roughly 2048..4096-bit RSA with exponent at least 65537. This is a compatibility limit to resolve for larger/legacy keys, not a declaration that the old application rejected them.

`KeyStore::prepare_for_system` unlocks an imported key in memory, generates a fresh random passphrase and writes only an encrypted OpenSSH identity into the private directory. `PreparedIdentity` exposes a host-only path and passphrase. Keep it with the owned SSH process, broker that exact identity's passphrase privately, then call `cleanup()` after process-tree shutdown succeeds. Failed cleanup retains the encrypted artifact for a retry. Never put a passphrase in argv, environment variables, logs or RPC data.

Legacy DSA and traditional `Proc-Type`/`DEK-Info` encrypted PKCS1/SEC1 PEM remain cryptographic parity work. `KeyStore::import_preserving_legacy` now retains explicitly selected originals and `KeyMetadata` under the private store, with `cryptographicallyValidated=false` and an explicit limitation. It does not fake validation or silently discard those keys. Six independently generated legacy fixtures cover that preservation path. Modern OpenSSH may disable DSA; enabling obsolete signatures still requires a user policy decision.

## Trust and retained OpenSSH

`KnownHosts::verify` requires a local explicit confirmation callback for unknown keys. Display host, port, type and SHA256 fingerprint. Reject and changed-key results never authenticate. Exact matches skip prompting; changed or corrupt trust never asks for automatic replacement. The callback runs outside the lock, followed by a locked recheck before saving. Concurrent conflicting approvals cannot replace the first accepted key. Duplicate JSON trust entries fail closed.

`system::SystemSshAdapter` is the retained OpenSSH preparation adapter. Its executables are explicit host configuration. `prepare_launch` performs a bounded public `ssh-keyscan` only on a user-requested connection, verifies the selected key through `KnownHosts`, then returns a typed strict pinned OpenSSH invocation without starting a process. The service uses `prepare_launch_guarded` and guarded store mutations to recheck current authority after lock waits. It uses application-private config/pin files, `StrictHostKeyChecking=yes`, an exact `HostKeyAlias`, no user/global trust fallback, no host-key updates, no local commands/proxies/implicit forwarding or default identity-file discovery. A scan is not proof of identity. The actual SSH handshake must match the approved pin before authentication. A concurrent key change therefore still fails closed.

The host consumes `SshLaunchRequest` through the existing service runtime and `PtyCommand`, never a raw SshSession wrapped in a second runtime/parser. Arguments, curated locale environment and PTY dimensions are typed. Saved credentials remain separate host-only fields. New service modules implement private OS askpass IPC and expose an injected `OwnedRuntimeStarter` for the owner registry. The request carries an unpredictable remote-command authentication marker; status remains Authenticating until matching-generation runtime bytes prove that command ran after authentication. Spawn/Runtime::Started is never Ready. Remote process lifetime follows normal SSH PTY hangup semantics; there is no exact remote-PID containment claim. See [the service assembly contract](SERVICE.md).

`transport` also defines a reviewed-library adapter boundary that gates authentication on trust. `UnavailableTransport` is an explicit error implementation, not the production system adapter. Its tests deliberately return errors rather than fake remote PTY success.

## Verification

```text
cargo test --locked -p shellfox-ssh --tests
cargo clippy --locked -p shellfox-ssh --all-targets -- -D warnings
cargo fmt -p shellfox-ssh -- --check
```

Windows tests require the real project helper at `tmp/tailscale/win32-x64/shellfox-tailscale.exe`, or an absolute `SHELLFOX_SSH_TEST_PERMISSIONS_HELPER` override. Key fixture generation additionally requires repository Node dependencies. It evaluates only the repository's independent test encoder and writes generated keys inside a new ACL-protected `tmp/ssh-rust-*` fixture, removed afterward. It never prints private keys.

Local evidence covers Windows storage ACLs, atomic failure behavior, cross-handle concurrency, corrupt-store preservation, hostile limits, trust races and 62 independently generated key fixtures. SSH scans and owned PTY launch are injected in adapter tests. No network connection, account login, production SSH config read/write, release or real remote shell test was performed. Native Linux/macOS, Windows arm64 and mobile/host integration acceptance remain separate gates. The integration owner runs TypeScript/native baseline checks to avoid duplicate full-suite runs.

See [UPSTREAM.md](UPSTREAM.md) for attribution, exact crypto pins and audit limits.
