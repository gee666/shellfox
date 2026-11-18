# Sources and dependency review

The profile/endpoint/PuTTY/known-host/PTY conversion logic was translated from this repository's active MIT-licensed files:

- `src/main/ssh/{storage,keys,known-hosts,pty,client,presentation}.ts`
- `src/shared/ssh-{schemas,endpoint}.ts`
- `tests/fixtures/ssh-test-keys.ts`, used as an independent runtime fixture encoder

Copyright c 2026 Shellfox contributors. The root [MIT license](../../LICENSE) applies. The port does not copy private keys or user configuration. PuTTY v2/v3 envelope layout, HMAC authentication and Argon2 bounds follow the existing implementation and PuTTY's documented key-file format. Cipher/KDF/private-operation algorithms come from the libraries below, not new implementations.

## Pinned crypto inputs

| Crate | Exact version | License |
| --- | --- | --- |
| ssh-key | 0.6.7 | Apache-2.0 OR MIT |
| aes | 0.8.4, zeroize | Apache-2.0 OR MIT |
| cbc | 0.1.2 | Apache-2.0 OR MIT |
| hmac | 0.12.1 | Apache-2.0 OR MIT |
| sha1 | 0.10.6 | Apache-2.0 OR MIT |
| argon2 | 0.5.3, zeroize | Apache-2.0 OR MIT |
| pkcs8 | 0.10.2, encryption/pem/sha1-insecure | Apache-2.0 OR MIT |
| pkcs1 | 0.7.5, alloc | Apache-2.0 OR MIT |
| pem-rfc7468 | 0.7.0, alloc | Apache-2.0 OR MIT |
| p256 | 0.13.2 | Apache-2.0 OR MIT |
| p384 | 0.13.1 | Apache-2.0 OR MIT |
| p521 | 0.13.3 | Apache-2.0 OR MIT |
| ed25519-dalek | 2.2.0 | BSD-3-Clause |
| ring | 0.17.14 | Apache-2.0 AND ISC |
| num-bigint-dig | 0.8.6, zeroize | Apache-2.0 OR MIT |

Cargo.lock belongs to the integration owner. The crate manifest pins the crypto versions above; root resolution and full dependency/license acceptance remain parent-owned. Review covered parser API use, bounded allocation/work, private/public consistency, authenticated PPK padding/comment handling and secret-bearing process arguments. This is a scoped source review and contract-test result, not a claim of an independent cryptographic audit.

The `rsa` feature of ssh-key is deliberately disabled. RustCrypto rsa 0.9 has unfixed RUSTSEC-2023-0071 concerning private-operation timing. The port uses ssh-key's format decoder and ring's RSA private-operation validation instead. `cargo tree --locked -p shellfox-ssh` showed no rsa dependency. A root `cargo audit --no-fetch --file Cargo.lock` run used a cached database with 1239 advisories and reported a retained rsa 0.9.10 lock entry plus unrelated unmaintained-crate warnings. The integration owner was notified to prune stale entries or identify another owner. That run was not a clean workspace audit and did not fetch a fresh advisory database.

SHA1 remains only for PuTTY v2 compatibility and legacy PKCS8 PBKDF2-HMAC-SHA1. Host fingerprints use SHA256. The retained OpenSSH adapter requests RSA SHA2 host-key algorithms, not SHA1 signatures.

## Platform responsibilities

The Go helper is the existing `native/tailscale/private_permissions*` implementation. Windows protection must use its protected current-owner DACL checks, not an alternate chmod or permissive mock. Helper executable provenance and packaging belong to the host. Tests use only its offline permissions mode.

The retained OpenSSH executable and ssh-keyscan executable are configurable absolute host paths. Packaging must pin/verify their provenance and platform availability before deployment. The adapter disables user SSH config/default identity-file discovery and owns its pin/config files; it does not modify system or user trust.

`PreparedIdentity` stores an encrypted OpenSSH conversion with a random passphrase, never a decrypted conversion. The host askpass broker owns that transient credential and its process lifetime. It must send credential responses only through a private authenticated local channel and retain failed-cleanup ownership. Android/iOS remain remote-only and must not link this host capability crate.
