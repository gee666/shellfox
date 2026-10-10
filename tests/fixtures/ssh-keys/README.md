# Generated SSH test keys

These RSA, ECDSA and Ed25519 keys were generated solely for the automated SSH tests with PuTTYgen 0.83. They are public test fixtures, not deployment credentials or keys copied from a user's system.

- `*-v2.ppk` and `*-v3.ppk` are encrypted using the deliberately public passphrase `fixture-passphrase`.
- `*.openssh` is the equivalent unencrypted OpenSSH test key.
- `src/main/ssh/keys.test.ts` compares public keys and verifies signatures across the formats.

Never authorize these keys on a real server. The in-process fixture server exists only during tests and does not read the user's accounts or SSH files.

To regenerate with PuTTYgen, generate each type into a v3 PPK, convert it to OpenSSH with an empty new passphrase, and re-encrypt a v2 copy using `--ppk-param version=2 --reencrypt`. Keep temporary passphrase files under the project's `tmp/` directory.
