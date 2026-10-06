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
