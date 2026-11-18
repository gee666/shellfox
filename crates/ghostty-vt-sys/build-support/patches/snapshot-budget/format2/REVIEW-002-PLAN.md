# Reagan changes for experimental-002

Status: plan implemented in checkpoints/experimental-002, awaiting independent
review. Passed new regressions/C proofs are not acceptance. experimental-001 is
immutable and is NOT accepted as
a bounded codec. Native002, corrected P4-002, normal Legacy/default availability
and staged artifacts are unchanged. Fix only the working overlay/own extraction;
publish a new versioned candidate after new exact-source and C proofs.

## Source residency before access

Move a bounded walk of ALL primary/alternate page nodes immediately after handle
validation, before configuration, profile capture/apply, layout validation or
metadata/cell access. Use only non-restoring `pageIfResident`; no `node.page()`.
Reject compressed source pages without rehydration or source mutation. Test with
real compressed history: preserve compressed nodes/data/accounting and parser
State, reject encode with no caller output publication, then compare source
future behavior to an untouched control. No validator may run before this walk.

## Canonical concrete tables before native increments

Distinct encoded IDs must not alias equal concrete style or hyperlink values.
Perform an allocation-free canonical-table pass before native `styles.add`,
`insertHyperlink` or cell reference increments. Styles compare canonical fixed
bytes; hyperlinks compare full canonical kind/ID/URI values. Rescan earlier
bounded entries if needed rather than allocating native sets to discover aliases.
Charge comparisons before they occur, using checked arithmetic. Reject aliases
in the preallocation pass, including many IDs for one value and managed cursor
references. Keep encoded-ID uniqueness/reference validation and the existing
physical per-page u16-refcount bound. Test both style and link aliases with valid
CRC/framing, heap/mapping counters zero on refusal, and unchanged source/output.

## Capacity-dependent work

Introduce one small checked work helper for conservative capacity × operations ×
comparison/copy bounds. Charge BEFORE native source lookups, table insertions,
grapheme append/map operations, hyperlink assignment/map operations and cursor
style/link restoration. Native page/table memory or layout capacity bounds may
conservatively overcharge; visible cells or payload bytes alone are insufficient.
Apply the same prepayment in allocation-free admission where counts/capacities
are known, so a tiny work budget refuses before native allocations. Include
canonical alias comparisons/rescans, row/physical/allocated-capacity traversal,
managed strings and all passes. Test sparse payload/high capacity, repeated
managed operations, overflow and deterministic low-work refusal.

## Writer-limit classification

Catch fixed staging writer exhaustion at the encoder boundary. Preserve real
allocation/budget failure priority. Distinguish internal output-capacity,
encoded-byte-policy and continuation-byte-policy errors. Return OUT_OF_SPACE for
caller capacity, LIMIT_EXCEEDED for encoded/continuation policy. A continuation
writer truncated by the output/encoded stage must not be mislabeled as a smaller
continuation cap. Add mid-buffer capacities, exact boundary, too-small encoded
policy and independent continuation cap tests, not just capacity1. Every refusal
keeps caller bytes unchanged and written0.

## Build/profile restrictions and cleanup

Require the existing supported-build/snapshot/slow-integrity restrictions and
matching compiled profile/budget declarations before entering the codec. Keep
both normal default and public Legacy gates OFF. Test slow-integrity and
snapshot-disabled builds explicitly rather than assuming they are compatible.

Add allocation-refusal sweeps and malformed late restore cases over managed
style/link tables, repeated graphemes, cursor hyperlinks, primary/alternate
screen replacement and saved/virtual cursor state. Verify no callback, partial
terminal/artifact, leaks or abort. Narrow regression tests first, then the full
38-case/214-cut codec/suffix oracle, Native raw1 byte fixture, combined selected
suites and actual default-OFF/experimental public C consumers.

## Publication

Generate experimental-002 with fresh patch/source/header digests and externally
derived experimental/default build identities. Reapply exact frozen1/2/3,
corrected P4-002 and NEW P5 in a fresh private extraction, fuzz0, and compare the
built source. Publish manifest/directory atomically after proof. Keep 001 files,
hashes, binaries and historical logs separate and do not relabel their proof.
No external exploit tooling, app wiring, SFM or release changes are authorized.
