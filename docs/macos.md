# macOS native helper builds

macOS uses a read-only process snapshot helper and a per-tab lifecycle supervisor. The native supervisor, TypeScript private-control client, backend lifecycle wiring and discovered-profile preflight are implemented. Profiles become available after the shipped supervisor passes compatible native self-preflight. A missing development binary is a build dependency failure, not a deliberately unsupported profile. Source/build configuration is not native acceptance. This Windows x64 host has no macOS SDK, so compilation, signing and actual Darwin launches cannot be verified here.

## Build and package paths

Build on native macOS x64 or arm64 with matching Node 24, pnpm 12.8.1, Xcode Command Line Tools and the macOS SDK.

- `src/main/terminal/native/darwin-process-snapshot.c` produces `shellfox-process-snapshot`.
- `src/main/terminal/native/darwin-terminal-supervisor.c` produces `shellfox-terminal-supervisor`.
- Development artifacts are `tmp/terminal-native/darwin-<arch>/<basename>`.
- Packaged artifacts are `Shellfox.app/Contents/Resources/terminal-native/<basename>`, outside ASAR.

`pnpm build` compiles at build time using `xcrun --sdk macosx clang`, C11, strict warnings, `-lproc` and explicit `-arch x86_64` or `-arch arm64`. The helper deployment target is macOS 13. Every build replaces previous helper outputs. Missing source/toolchain or failed preflight fails the build. There is no runtime compiler invocation or development-path fallback in a packaged app. End users do not need clang, Python or a .NET runtime for Darwin helpers.

```sh
pnpm install --frozen-lockfile
pnpm typecheck
pnpm test:unit
pnpm test:packaging
pnpm build
pnpm test:terminal-native
pnpm test:storage
pnpm test:pty
pnpm package
pnpm test:packaged
pnpm test:packaged:startup
pnpm make
```

Source compilation for node-pty/SQLite, if needed, also needs Python 3. Helper compilation still runs even if Node-API addon prebuilds are used. Build helpers separately with `pnpm build:terminal-native`.

## Preflight and ownership

The snapshot helper uses SDK libproc and numeric sysctl MIBs, not dotted sysctl CLI PID OIDs or parsed ps text. Its capability response requires exact self birth/credentials, argv/environment marker and enumeration evidence. Metadata failures stay unknown. It is read-only and cannot itself authorize safe shutdown.

The separate supervisor remains the PTY session leader, with the interactive shell in a foreground job-control group. Its private same-user control token stays in environment only and is stripped before the user's shell starts. The backend requires compatible supervisor readiness before a macOS profile can launch. It tracks the authenticated real shell child, not the supervisor transport. Close uses the private socket and waits for actual wrapper exit; a timeout closes only the client connection, never kills the wrapper. Missing/failed helpers produce unavailable reasons, not fake healthy terminals or numeric-PID termination fallback.

Packaging tests check native architecture, executable mode, code signatures and real helper capability preflight at the packaged resource path. They invoke both typed backend packaged-path/capability checkers and verify that neither missing packaged helper can use a development binary. A separate stamped-source backend fixture loads the actual packaged node-pty and exercises real launch/output/resize/replay/exit/close, including the packaged supervisor on Darwin. These gates must run on both macOS runner architectures; none has run on this Windows host. CI also enables the backend's actual Darwin supervisor fixture with `SHELLFOX_DARWIN_SUPERVISOR_NATIVE=1`, testing real job-control groups, descendant cleanup, unrelated-session isolation, secret stripping and exit 7. The normal unit run skips this opt-in native case.

## Cleanup snapshot regression

The supplied 0.7.0 arm64 release log, `tmp/release-070-mac-arm64.log`, shows successful helper compilation/preflight, package signature verification and raw native PTY smoke. The backend fixture then fails confirmed close with `Owned session membership or credentials are inaccessible or changed.` This is a supervisor cleanup refusal, not evidence of a missing addon or bad signature. The old message does not identify the failing PID, syscall or check, so it cannot establish the exact native trigger.

Cleanup previously retried unreadable snapshots while leaving previously frozen groups stopped. A fork/exec/exit transition overlapping the freeze may need those groups to run before a new snapshot can succeed. Repeating the read cannot fix that state. The additional 100-pass limit also cut fast failures short of the existing five-second freeze budget. Commit `faf93e7` added read retries but did not restore the groups between them; `d4a5d9c` handled the separate control-reply/normal-exit race.

Cleanup now rolls back every reversible freeze after an unreadable or incomplete frozen snapshot, resets stability and takes fresh evidence within the original deadline. It still requires two authenticated fully frozen snapshots before K. Permission failures, unknown membership, UID changes, saved UID changes and PSUGID processes never authorize termination. A failed rollback or expired deadline retains ownership. No numeric-PID/group fallback was added. Birth reads also bracket the final SID lookup, and both credential reads must pass, including an unchanged PSUGID flag. Refusal replies include bounded stage/PID/errno diagnostics without argv, environment or secrets. The packaged smoke labels which close failed and still requires its first close call to succeed.

`node scripts/darwin-supervisor-model-test.mjs` compiles the actual production snapshot and cleanup functions against deterministic C OS stubs, using a C11 compiler on Linux/macOS. It covers freeze/read/rollback ordering, incomplete fork state, recovery beyond the old pass limit, persistent denial, failed rollback, birth recycling across SID lookup, credential/group changes, foreign sessions and verified absence. All generated files stay under `tmp/darwin-supervisor-model/`. This is a control-flow regression test, not a macOS syscall or signal test.

The opt-in Darwin PTY fixture also closes five newly authenticated `/bin/zsh -l -i` sessions immediately, with no close retries or startup delay. The release workflow now runs this fixture on both Darwin architectures in addition to the unchanged packaged backend smoke. Native validation of this patch remains required. Passing the C model establishes the liveness defect and its repair, but does not prove that defect caused this particular CI run.

## Signing

Build artifacts receive ad-hoc signatures for local development. Forge stages executable helpers before Packager's inside-out signing pass, including resources, unpacked native addons and node-pty's spawn-helper. No signed resources are added or modified afterward.

The default app signature is ad-hoc, suitable for local preview testing, not trusted distribution. Optional certificate signing requires an already-installed identity:

```sh
SHELLFOX_MAC_SIGN_IDENTITY='Developer ID Application: <name> (<team>)' pnpm make
```

`SHELLFOX_MAC_SIGN_KEYCHAIN` can name a keychain. The identity is validated; failure is fatal. Certificate mode enables hardened runtime and timestamping. Native C helpers receive no Electron JIT entitlements; Electron code uses Packager's standard signing entitlements.

This does not import certificates, change keychains, notarize/staple, or promise Gatekeeper acceptance. Do not distribute these previews as a notarized release. App Sandbox/Mac App Store and universal binaries are not configured.

## Native acceptance still required

Verify real shell/job-control behavior, descendant cleanup, session/Settings retention, failed-close ownership retention and native quit cancellation/confirmation on both macOS architectures. Test missing/corrupt/mismatched helper binaries, inaccessible identities, terminal-group changes, shell exit with pending descendants and actual packaged signatures. A passing source scan, mocked test or Windows package cannot substitute for these results.
