# Windows keep-awake rollback guardian

This is a separate, non-UAC capability in the active helper. Legacy `probe`,
`launch`, and isolated-launch protocols are unchanged. The guardian starts no
children and executes no PowerShell, scripts, account profiles, or commands.
It is not wired to the Rust power factory yet. The existing Rust Windows backend
remains partial, and probe/fake-test success is not full application acceptance.

## Safe capability query

`probe-power` emits exactly this JSON object followed by a newline. It makes no
power, mouse, journal, or owner-process calls:

```json
{"protocol":"shellfox-power-guardian","version":1,"display":true,"system":true,"awayMode":true,"idleMouse":true,"lidRollback":true}
```

The fields describe implemented operations, not permission to apply them on a
particular machine. Group policy, hardware, or Windows can refuse an operation.
The helper never elevates to work around a refusal.

## Trusted private transport

Spawn `power-guardian` directly from the trusted local host with inherited private
stdin/stdout pipes. No shell or UAC. The Rust factory must authenticate the complete
published helper asset set and retain the original returned helper process handle
and birth. Supply a minimal launcher environment without `DOTNET_`, `COMPLUS_`,
`CORECLR_` or `COR_` hooks/overrides. A C# entry point cannot sanitize runtime
startup hooks that execute before it. The managed DLL, dependencies, runtime configuration and native runtime
assets matter, not just the apphost EXE. Rebuild the trusted packager/source
manifest instead of hardcoding hashes or trusting sibling files on sight.

The guardian must be outside every Job. It rejects any enclosing Job before GO,
including benign Jobs, rather than guessing about nested kill-on-close policy.
Do not attach it to the host/terminal Job or kill it on host exit. A host in a Job
must use an explicitly permitted breakaway launch, or report this capability
unavailable. Never fall back to a guardian that dies before rollback.

Duplicate the host's original current-process handle with
`PROCESS_QUERY_LIMITED_INFORMATION | SYNCHRONIZE`, inheritable only through the
explicit launch handle list. Include only intended pipe/owner handles and any
verified bundle lease handles needed after host death. Host-owned file pins alone
are not enough for this long-lived guardian, see [the loader review](SECURITY.md).
Do not supply a PID to be reopened. The guardian retains that original handle, verifies
its exact creation FILETIME and rejects a self handle. It uses a kernel wait on
that handle, never PID polling, tree killing, or a PID-reuse fallback.

Send bounded NDJSON with exactly these fields:

```json
{"type":"power-init","version":1,"root":"C:\\private-data\\power","ownerHandle":"123","ownerBirth":"134355986344672889","nonce":"0123456789abcdef0123456789abcdef"}
```

`ownerHandle` is a decimal handle value in the guardian's inherited handle table.
`ownerBirth` is a positive decimal Windows FILETIME. Both are strings to avoid
JSON integer precision loss. `nonce` is exactly 32 lowercase hex characters,
chosen fresh by the trusted host. Unknown/duplicate fields and versions fail
closed. Initial input and the subsequent GO have separate ten-second deadlines.

The helper validates identity and the private root, acquires its leases, and emits:

```json
{"type":"power-ready","version":1,"nonce":"0123456789abcdef0123456789abcdef","pid":1234,"birth":"134355986344672999"}
```

Rust must check this against its original guardian process handle. No power or
mouse change happens until the private GO message:

```json
{"type":"power-go","nonce":"0123456789abcdef0123456789abcdef","action":"enable"}
```

Only an explicit local USER toggle may send `enable`. Do not auto-enable from
startup, remote RPC, renderer-supplied commands, or persisted enabled state.
For next-startup journal recovery, use `action:"recover"` instead. Recovery only
restores owned settings; it never acquires awake requests or sends mouse input.
The factory must run that recovery action on the same persistent power root at
startup even when keep-awake remains OFF.

After successful enable:

```json
{"type":"power-active","version":1,"nonce":"0123456789abcdef0123456789abcdef"}
```

All requested operations must have succeeded. A failed startup rolls back rather
than claiming active or silently degrading to the basic Rust backend.

Stop with `{"type":"power-stop","nonce":"0123456789abcdef0123456789abcdef"}`.
EOF, malformed commands, duplicate GO and original-owner death also stop it.
After GO, confirmed rollback emits:

```json
{"type":"power-stopped","version":1,"nonce":"0123456789abcdef0123456789abcdef"}
```

Errors are `{"type":"power-error","version":1,"nonce":"...","code":"failed"}`
or the same shape with `code:"rollback-pending"`. No dynamic diagnostics or paths
are returned. A failed operation can produce an error followed later by a valid
stopped acknowledgement. An exit code, stdout EOF or missing acknowledgement
alone is not rollback proof. Before GO, cancellation exits without a stopped
acknowledgement and leaves any stale journal for authorized recovery.

## Operations and rollback ownership

Enable creates owned display, system, and away-mode power requests using the
native Windows API. Closing the request handle also releases partially acquired
requests. Every 30 seconds it submits a tiny paired relative mouse move, as the
Electron helper did. A failed input operation triggers rollback. Mouse input
stops immediately on STOP/EOF/death, including while restoration is pending.

The helper captures the actual active scheme GUID and both original AC/DC lid
indices. It persists and flushes a write-ahead intent before changing each
nonzero index to DoNothing. Already-zero indices are never claimed or written.
It never uses SCHEME_CURRENT for writes. On restoration it writes an original
value only if that owned scheme/index still equals its applied zero. A different
value belongs to a later edit and is left alone. If the active plan changed, it
restores only its own unchanged indices in the saved inactive scheme and does
not switch the user back. Effective-setting refresh is conditional on that exact
saved scheme still being active.

Windows offers no atomic compare-and-set for power scheme settings. There is a
small query/write or query/refresh race with external tools, and a later edit to
the identical zero value is indistinguishable from our own write. These guards
prevent unconditional stale-value restoration; they are not exclusive control
over the user's power settings. Native acceptance must review these OS limits.

Rollback failures retain the journal, root lease, machine-wide guardian mutex,
and any held awake requests. The guardian retries every 30 seconds, even after
the host has died. It is not killed by a timeout or watchdog. Only completed
rollback deletes the journal and releases requests. Forced guardian termination
leaves the durable journal for the next trusted startup. Keep the root stable;
do not move, discard, or replace a root that has unresolved rollback state.
Multiple roots must not be used to bypass pending recovery. A machine-wide mutex
serializes cooperating guardians, while the exclusive root lease prevents a
second guardian from adopting a live journal.

## Private durable state

The caller prepares an existing directory on a fixed local volume. Use ordinary
absolute drive paths, not UNC/device paths, dot components, ADS or ambiguous
trailing dots/spaces. The root must have a protected DACL, be owned by the current
user SID, and grant access only to that SID and SYSTEM, with owner full control.
For example, construct a Windows security descriptor equivalent to
`O:<SID>D:P(A;OICI;FA;;;<SID>)(A;OICI;FA;;;SY)` when creating the dedicated root.
The helper never repairs permissions on arbitrary supplied directories.

Directory ancestors are opened without following reparse points and pinned
against replacement. `guardian.lock` and `rollback.log` are exclusively opened
with no-follow semantics. Their ACL/owner, regular-file type and single-link
count are checked through the opened handles before writes. Created files have
explicit private descriptors. No account or global settings file is used as a
journal. These protections are not a hostile same-user/admin sandbox.

The bounded append-only journal contains typed immutable scheme/original-value
identity and per-index ownership/refresh state. Each frame has a length and
SHA-256 for corruption detection, followed by `Flush(true)` before mutation.
An incomplete tail recovers the last full intent. A corrupt complete frame,
unknown schema, or oversized journal fails closed and is retained. Recovery
handles crashes before/after each write or ownership update. Journal removal is
by the original validated handle only after successful rollback.

## Allowed checks and remaining gates

```text
node scripts/dotnet-sdk.mjs provision
node scripts/dotnet-sdk.mjs test
node scripts/terminal-native.mjs --arch x64
node --test scripts/power-guardian.test.mjs
node scripts/terminal-native.mjs --arch arm64
```

The active helper/tests use supported .NET 10, provisioned only under project
`tmp/`. See [the runtime and loader review](SECURITY.md). The tests inject every
power/input operation. They exercise controller lifecycle,
30-second pulses, durable intent ordering, original-plan ownership, external
edits, partial startup, failed restore retention/retry, next-startup recovery,
strict protocol parsing, ABI layout, and private owned filesystem fixtures under
project `tmp/`. Fixtures are removed after each test. Node tests run safe probes
only. They never execute the guardian mode or change power plans, lid actions,
mouse position, or account state. Arm64 publishing is compile-only here.

Real guardian mode, hardware/policy behavior, host crash, forced guardian death,
reboot recovery, breakaway/Job behavior and UI/local-toggle factory integration
still require separately authorized native acceptance. Do not run the Rust
`windows_real_request_is_brief_and_released` test or a full power suite under the
no-global-effects check restriction. No release or installed-app change is part
of this work.
