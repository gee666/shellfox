# Native storage fixture coverage accounting

This is source/metadata accounting, not native acceptance. Current restrictions prohibit executing Service-lib, helper/Go and network fixtures. A compiling fixture is not a passing runtime test.

## Actual bootstrap fixture

`remote_setup_tests::native_boot_retains_local_only_envelope_before_any_remote_activation` uses the cfg-test-only private `NativeBootHarness`. It calls the real native acquisition, original-handle/control-child binding and the same production retained-envelope preparation method. It does not construct a fake NativeControlChild or expose a production test API.

Its assertions now require the exact typed static LocalOnly reason NativeApprovalStoragePending, the retained envelope, and no optional remote assembly/bind/backend/runtime, local actor/endpoint or observer-factory activation at setup time. An arbitrary preflight/auth/IO error fails the fixture rather than passing as the gate. Assertions are captured until after actual harness cleanup. The fixture remains ignored and COMPILE-ONLY. The final availability Remote/Service/Desktop/CLI test metadata check passed in 6.04 seconds and matching strict --tests Clippy passed in 9.77 seconds on Windows. These compile/lint checks did not execute fixtures. Logs are tmp/native-local-only-metadata.log and tmp/native-local-only-clippy.log. The removed positive signed-backend factory assertion is PENDING, not counted as a replacement pass. Actual local readiness occurs later through the same BootGuard, not at this setup outcome. Go path-based state/config acceptance remains PENDING.

## Leaf storage fixtures are not ownership proof

Musa's metadata/store fixtures use real Files native issuers and sealed directory receipts. They cover leaf storage behavior only. They do not prove Go acquisition, Service ownership, BootGuard cleanup, networking or Host/VFS confinement.

`native_preference_leaf_roundtrip_does_not_initialize_journal_or_uploads` is leaf preference coverage. It does not replace the former positive backend/factory/journal bootstrap coverage.

## Approval write gate

Approval WRITE is intentionally gated. The retained gate regression is `gated_save_does_not_create_marker_stage_or_change_approvals`, asserting Unsupported and no marker/stage/approval mutation. `v1_needing_save_cannot_install_a_normalized_grant_while_writes_are_gated` retains the normalization refusal boundary.

Positive approval persistence, atomic recovery and crash recovery acceptance are PENDING. Removed historical success fixtures are NOT counted as passing replacement coverage. General prefs/journal durability policy has not been relaxed by ephemeral endpoint readiness.

## Preserved policy regressions

The source retains:

- `readonly_v2_keeps_exact_namespace_and_explicit_full_account_policy`.
- `every_legacy_pending_presence_denies_without_repair_or_deletion`.
- `ownership_loss_denies_read_and_keeps_preparation_owner_for_disposition`.
- FullAccount policy tests in `approval_policy_tests`.
- Native preference read checks against current ownership, separately from the mutation fence.

Their current execution status must come from their owning worker's explicit evidence, not this document or leaf fixture substitutions. Under this native migration checkpoint, Service/helper fixtures are compile-only.

## Remote admission target semantics

When typed local-only readiness reports the remote feature unavailable, the absent REMOTE admission target returns the existing unsent NativeAdmissionError::Stopped. This says nothing about local Host/actor liveness. It never authorizes host reacquisition, cold fallback or mutation replay. Only unsent Full admission is retryable. Settings forwards that error to its remote error state; local metadata and terminal routes use the same owner.

The focused owner fixture asserts no remote owner task was enqueued and the same sender still admits local Status. The AppHost fixture preserves ReadyLocalOnly's reason, admits local windows/counts and still requires Stop rather than treating it as NoOwner. These mechanical fixtures are compile-only and are not Go/Service ownership acceptance.

## Remaining positive gates

- Actual private NativeBootGuard bootstrap/cleanup execution.
- Positive approval WRITE/persistence and crash recovery.
- Native journals/uploads/editor creation and recovery.
- Go library state/config anchoring and runtime acceptance.
- Host/VFS review and Unix stale-socket restart/accumulation policy.

Temporary path-only refusal constructors may be removed only after code references migrate. No working path fallback is permitted.
