# Windows atomic private-directory anchor prototype

Status: private `cfg(all(test, windows))` child of DirectoryPlan. Sixteen scoped owned Windows candidate/prototype kernel/lifecycle tests and strict Files all-target Clippy passed. Serial and parallel reruns are the same sixteen tests, not thirty-two independent tests. Independent review is required again. Production parent policy, Root/UI composition and default support remain unchanged and unaccepted.

## Original authority and keeper

`WindowsAtomicCreationAnchor` is built only from the actual original RW DirectoryPlan with exactly one originally missing leaf. Its scope, full identities and retained ancestor handles come from that plan. No File/bool/path issuer, public mint, second ledger or wire constructor exists.

The original `DirectoryPreparationOwner` and plan remain outside unwind. Creation reserves and begins a row in the same authority's bounded `plan_creations` ledger before the native call. Native-return capture uses a preallocated `CreationCustody` payload outside an inner catch boundary. The native File and full identity are moved into that original row before any fallible clone, current-private-object validation or injected post-create fault. The committed File is the actual returned File, not a pathname reopen.

Native errors move the original parent/leaf/evidence and any returned File into the pre-registered ledger row before returning a typed `DirectoryPreparationFailure`. A panic during raw capture similarly stores that custody before resuming unwind. The capture payload is transient native-return storage, not the lifetime keeper. Post-registration errors and unwind leave the actual created File with the external original owner.

The caller retains the original mutation guard outside catch boundaries. This prototype neither poisons nor clears the global mutex. It does not construct a cleanup scope or unlink after an error.

## Creation, current verification and handoff

The distinct test-only branch uses handle-relative `NtCreateFile(FILE_CREATE)`, a complete protected current-user owner/DACL at creation, and the actual no-DELETE-access/sharing3 directory handle. The fixture deliberately grants untrusted inheritable mutating mask `0x001301bf` on the parent.

`PinnedPrivateDirectory` is a private current verified custody view, never a birth receipt. Verification checks the current private object on the actual held handle, its full identity, retained ancestor identities and a no-follow relative proof open. Later loss of protected-private status rejects verify, reopen and handoff without repair.

Handoff revalidates the original plan, confirms the exact full identity and uses its original authority's private derivation. The derived root keeps the same ledger and inherited revocation restrictions. Original missing-plan adoption works only while the original ledger still holds its created entry. Stale preservation ends that entry's adoption proof.

## Existing private-root branch

A separate private reopen method requires an originally existing DirectoryPlan under the same RW scope. It verifies current privacy on the original held child, opens RW relative to the exact original parent/leaf with an actual noDELETE/share3 pin, compares full identities, and revalidates the original plan. It does not repair ACLs, issue a birth/CreatedPrivate receipt, or add a created-ownership ledger row.

This is a test-only original-plan branch. Generic existing-object verification still rejects the same untrusted-mutating parent. No production verifier, native primitive, Root policy or UI path changed.

## Explicit pin lifetime and preservation

Local custody views, original ledger entries, existing plans and derived roots hold independent real handles. Releasing a view does not close the original created File. Preserving the original keeper does not close a still-held derived root or existing-plan/reopen pin.

Tests observe native DELETE-access opens failing with `NTSTATUS 0xc0000043` while relevant pins remain, then succeeding after every relevant child pin is explicitly released or preserved. Child rename similarly succeeds only after release. Preservation leaves the artifact in place and retains original-parent/identifier evidence. It is not deletion authority or proof of NoOwner or external resource shutdown.

## Scoped evidence

The original ten prototype tests cover:

- Generic rejection and absence of the actual `generic-denied` leaf, original-ledger adoption and derived-root full identity.
- Independent real pins through existing reopen, derived-root handoff and explicit release/preservation.
- Existing current-state verification without a new created-ownership row or birth claim.
- Current-private-status loss rejecting verify/reopen/handoff without repair.
- Original issuer revocation fencing derived and reopened scope.
- Ordinary and real junction collisions without adoption or ACL repair.
- Pre-create revocation without reservation or native mutation.
- An injected post-registration hook error using the existing cancellation token, with actual native custody retained. This is not proof of a real caller/startup cancellation path.
- Injected post-registration diagnostic and privacy failures retaining the original created File.
- Post-registration unwind with external keeper/plan/guard, actual DELETE-open denial, and no global or ledger mutex poison.

The raw-capture catch arrangement is implemented as described above. Tests do not manufacture an ambiguous native return or force a full-identity query failure inside the native capture boundary. Those paths retain the original payload in the ledger before diagnostic return or resume, but are not claimed as separately injected kernel outcomes.

## Staged admitted candidate

Six additional candidate tests exercise the real fixed-domain DirectoryAdmissionGuard. The entry acquires Active before waiting for the actual global mutex, borrows its guard without reacquiring, keeps the original keeper outside worker catch, and finishes with typed structured effects. The tests cover multiple missing steps and live private-created prefix handles, existing current verification without a created row, foreign/ordinary-prefix rejection, post-effect errors/unwind quarantine, ActiveBusy preservation and terminal preservation without globally revoking ordinary reads.

Successful private-native capture assigns PrivateCreator only at the actual explicit-SD FILE_CREATE callsite, while generic commit remains Ordinary. It records creator provenance, not continuing privacy or a public birth receipt. Current binding/privacy checks remain mandatory.

The candidate has no real caller cancellation input. Hook errors/panic do not prove general cancellation, and revoke waits for MUTATIONS. Native capture query/partial-return branches remain source-only, not separately injected. The first timed-out check remains FAILED; owned old test guard scopes were corrected for the keeper's mutex-owning preserve operation before fresh passes. The log records the first launch's missing registered-PID exit proof. No global poison was cleared.

## Public contracts and remaining gates

- [NtCreateFile](https://learn.microsoft.com/windows-hardware/drivers/ddi/ntifs/nf-ntifs-ntcreatefile) specifies `FILE_CREATE`, handle-relative object attributes, creation security descriptor and share access.
- [CreateFileW sharing semantics](https://learn.microsoft.com/windows/win32/api/fileapi/nf-fileapi-createfilew) describe share-mode compatibility and withholding `FILE_SHARE_DELETE`.

All fixtures are current-account, owned repository `tmp/` objects. No other-account, privileged process-handle interference, global VFS, ReFS, Go, Service-lib, network or GUI acceptance follows from these tests. Unix parent-trust policy and existing creation/privacy gaps remain unchanged. Do not promote this prototype or enable default composition without the next independent creation/reopen/lifecycle review.
