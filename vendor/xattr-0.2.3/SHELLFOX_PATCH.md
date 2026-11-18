# Locked xattr 0.2.3 compatibility delta

Original crates.io archive SHA-256 (matches the root Cargo.lock before path override):
`6d1526bbe5aaeb5eb06885f4d987bcdfa5e23187055de9b83fe00156a821fabc`.

The archive was checksum-verified before extraction. Original LICENSE-APACHE and
LICENSE-MIT and all upstream package files are retained. Only `src/sys/mod.rs`
changes: Linux/Android's missing-attribute alias uses libc::ENODATA because current
libc no longer exports the ENOATTR alias there. Darwin/BSD keep upstream ENOATTR.
An in-module Linux/Android assertion covers the alias. No syscall, error-policy,
provider, package version or dependency change is intended.

Original archive/source, exact diff, provenance and complete vendor hashes are
retained under `tmp/app-composition/xattr-patch-001/`. No registry-cache source was
modified. This directory is outside the workspace `crates/*` member glob.
