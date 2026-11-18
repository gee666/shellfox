# Private staging and user-file publication policy proposal

Status: proposal only. The existing user-file destination-mode/DACL restoration order is NOT changed. Private-at-birth creation does not by itself close publication privacy.

## Confirmed distinction

Owner-private metadata can use the new private-native read/stage/publish helpers without broadening stage permissions. Existing destinations must verify owner-private; unknown or weak objects are not repaired. Failed pre-publication checks leave destination data unchanged and clean only an identified stage. Atomic publication or directory-sync errors must not trigger automatic data retries.

User text/image files have a separate contract: existing code restores the destination mode/DACL to the named stage before the final destination/authority checks and rename. If that grants another reader, a named unpublished draft may become readable before publication, including on a later abort. An initial0600/protected descriptor does not cure that later widening.

## Requirements to preserve

- No non-owner access to unpublished draft bytes.
- A pre-commit abort does not replace the destination or expose a widened stage.
- Preserve revision/identity/conflict checks, original parent/custody, no-follow and owned cleanup.
- Never silently narrow the final file's permissions, claim a failed save did not commit when it did, or retry an uncertain mutation.
- Do not weaken scope/revocation to implement permission repair.

## Proposed bounded policy choices

First classify owner-private metadata separately from user-file publication. Do not apply an owner-private metadata helper to an arbitrary existing user file and call permissions preserved.

For destinations whose security grants no additional untrusted reader, stage security can remain equivalent/private through the existing atomic publication. Effective-policy classification must itself be reviewed; unknown ACLs/groups are not assumed private.

For broader user destinations, choose an explicit platform publication policy before implementation:

1. **Fail closed until an atomic security-preserving replacement is proven.** Keep the stage private and return an explicit unsupported condition before changing destination data. Do not silently change final permissions to private. This is the safe strict-abort default if no satisfactory native primitive exists.
2. **Explicit commit with post-commit security completion.** Keep the stage private through atomic data publication, then apply the approved destination security to that SAME published handle under a reviewed state machine. This requires a distinct committed-but-security-incomplete outcome and durable recovery evidence. A post-commit error is not an abort. Existing generic save success/error shapes must not conceal it. No automatic second data write is allowed. Binding, authority loss and permission-completion failure must be specified before approval.
3. **A proven inaccessible preparation namespace with atomic publication**, only where the platform actually enforces it. On POSIX an owner-only private directory may protect a broader-mode child during preparation under the stated parent/trust model. This is not a portable Windows solution: ordinary Windows bypass-traverse privileges mean a private directory alone must not be assumed to hide a known broader-readable child. No random filename or untested filesystem assumption is a confidentiality proof.

Do not introduce path-based ReplaceFile calls, deprecated filesystem transactions or guessed rename/security flags to obtain a pass. Existing FD-relative native rename only atomically changes names; it does not prove atomic destination-security transfer.

## Review and tests before activation

Agree error/commit outcomes with the client/UI owner and original custody owner. Test abort after every permission/publication boundary, unchanged destination on pre-commit conflict/revocation, explicit committed outcomes, no visible widened unpublished stage, security-completion failure, replaced binding, exact owned cleanup and crash recovery. Cross-account/native security evidence requires separately approved environments. No such execution or acceptance is claimed here.
