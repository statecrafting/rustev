# Synthetic acquisition acceptance (spec 036)

These fixtures are synthetic mechanics examples, not catalogue quality claims.
`../acquisition.rs` owns the builders, ordered source replies, received-request
assertions and these new goldens. The existing package definition and goldens
are not regenerated. Set `RUSTEV_BLESS=1` only when intentionally updating these
fixtures.

## Acceptance map

Test names below are exact Rust test names. Unqualified lodging tests live in
`packages/rustev-pkg-lodging/tests/acquisition.rs`.

| Item | Proof |
|---|---|
| Specific succeeds, empty, or guarded out; origins and protected received fields | `lodging_specific_first_received_requests_origins_and_tier_goldens` |
| Protected relaxation golden | `lodging_reference_definition_and_protected_refusal` |
| Corroboration with acquired keys, hint-to-cached grade, absent parent and handler path | `lodging_guarded_subareas_corroborate_composed_key_or_take_skipped_handler`; core `plan_three_text_projection_checks_bounds_types_and_absence` |
| Five acquisitions, two paged, byte-identical replay and zero calls | eval `five_acquisitions_two_paged_reproduce_without_source_calls` |
| Each removed page | eval `removed_page_is_missing_evidence_with_inspectable_judgment` |
| Plan 3 snapshot 2 and judgment 2; Plan 2 replay 1 | eval `source_free_plan_three_snapshot_two_and_judgment_two_reproduce`, `plan_two_assembly_still_uses_identical_replay_one_bytes` |
| Calls, items, bytes, hard-cost refusal goldens | core `every_budget_ceiling_has_a_refusal_golden` |
| Every tier outcome in evidence 2, pages in run 3 | `lodging_specific_first_received_requests_origins_and_tier_goldens`; runtime `paging_records_each_page_and_matches_the_pure_core` |
| Two sources, first duplicate retained, origins | `lodging_two_sources_keep_first_duplicate_and_protected_requests` |
| Unmappable source | core `protected_groups_required_fields_and_mapping_refusals`, `exact_requests_types_cycles_and_source_mappings` |
| Paging stop, page/item caps, exhaustion and replay | core `paging_stops_and_counts_truncation`; eval `paging_stop_reasons_and_truncation_reproduce_without_dispatch` |
| Runtime expiry | `window_expiry_cancels_and_queued_pages_do_not_dispatch`, `widening_keeps_one_window_and_records_expired_later_tier` |
| Named retries, no retry on denied, contained panic | runtime `retries_only_named_classes_denied_never_retried_and_panics_contained`, `retry_delay_cannot_reach_the_window_close` |
| Cancellation kinds | runtime `cancellation_records_each_kind_and_ack_without_inventing_a_stop` |
| Cache opt-ins, age, scope, expiry and freshness | runtime `cache_requires_both_opt_ins_records_age_expires_and_isolates_scope`, `cache_hit_preserves_page_as_of_and_core_freshness` |
| All interleavings of three acquisitions | core `independent_pages_are_byte_identical_in_every_interleaving` |
| Widening order independence | core `widening_independent_supply_permutations_are_identical` |
| Compatibility | Existing crate golden suites; no existing golden bytes changed. Lodging `a_run_1_sink_refuses_a_plan_3_and_capture_writes_replay_2` (increment 4) replaces the former capture refusal assertion. |
| Mutation negative controls | Six `spec 036` seeds in eval's harness run the affected core, runtime or eval crate. `OWN_RUNS` excludes them from spec 004's unfiltered run. |

## Section 5 negative cases

| Cases in specification order | Test |
|---|---|
| Protected relaxation and patch | core `protected_groups_required_fields_and_mapping_refusals` |
| First offset page starts at zero | core `paging_stops_and_counts_truncation` |
| Failed second page rolls back pooled first page | core `widening_failed_tier_rolls_back_pages_and_duplicate_counts` |
| Required-field relaxation | core `protected_groups_required_fields_and_mapping_refusals` |
| Protected hint | core `protected_request_rejects_a_constructed_hint` |
| Model-derived request | core `exact_requests_types_cycles_and_source_mappings` |
| Missing or duplicate source and missing operation | core `typed_bounds_bindings_and_retries_are_rechecked`, `exact_requests_types_cycles_and_source_mappings` |
| Missing pool field or integer/decimal mismatch | core `protected_groups_required_fields_and_mapping_refusals`, `exact_requests_types_cycles_and_source_mappings` |
| Call ceiling, 4097 bound, estimated hard cost, non-idempotent retries, deadline shares | core `every_budget_ceiling_has_a_refusal_golden`, `typed_bounds_bindings_and_retries_are_rechecked` |
| Definition 2 without track constructs | core `a_definition_2_without_track_constructs_is_refused` |
| Descriptor without operations retains bytes/identity | contract `absent_operations_preserves_bytes_and_identity` |
| Missing skipped handler or propagating skipped | core `guarded_acquisition_requires_skipped_handling_except_for_presence_probes` |
| Absent guard | `lodging_guarded_subareas_corroborate_composed_key_or_take_skipped_handler`; core `unresolved_guards_and_requests_propagate_without_dispatch`, `skipped_policy_handlers_and_probe_guards` |
| Wrong identity, extra field, oversized page, future timestamp | core `invalid_pages_never_become_values`, `freshness_boundary_and_remaining_page_validation_rules` |
| Stale page | core `freshness_boundary_and_remaining_page_validation_rules` |
| Denied, panic, expired window | Runtime tests listed above |
| Skipped then empty | core `widening_limits_empty_and_all_skipped` |
| Failed then filled, or failed without filling | core `widening_failed_tier_rolls_back_pages_and_duplicate_counts` |
| Truncation and notice | core `paging_stops_and_counts_truncation`, `widening_truncation_is_visible_even_when_an_earlier_failure_prevents_resolution` |
| Invalid composed key | core `compose_normalization_faults_and_constructed_request` |
| Removed replay page | eval `removed_page_is_missing_evidence_with_inspectable_judgment` |
| Ambiguous retained page identity | eval `conflicting_pages_with_equal_identity_are_ambiguous_for_candidates` |
| Wrong prepared source identity | runtime `prepare_requires_exact_source_identity` |
| Plan 3 capture and Plan 2 compatibility | Eval and lodging capture tests listed above |
| Existing definition identity | Existing compile golden suites |

## Minimal implementation additions

The registry in `crates/rustev-core/src/ops.rs` has no exact operator that maps
or plucks a record field into a text list. The authorized fallback adds only
bounded text projection to plan 3 step references: `step:subareas/items/*/key`.
It preserves list order, duplicates, item bounds, text bounds and the selected
field's grade and lineage. Non-text, missing and malformed field projections
are refused; null or missing values do not silently disappear.

Spec 036 section 3.4.5 introduces acquired-value field references and its
Corroboration acceptance requires testing a composed key over acquired keys.
Spec 032 section 3.7.2 and ratified decision 10 define `member_ref` for referenced
sets. These are the basis for the owner-authorized projection extension; the
wildcard spelling itself is an implementation addition, not quoted spec text.
The reference uses `member_ref`; `member_of@1` and its literal-set meaning are
unchanged. No registry, wire schema or contract type changed.

Acquisition and widening guards now allow `present` to observe an explicitly
absent input before propagating input status. Other unresolved reads still
propagate, including a presence probe of a failed exact step. This realizes
spec 036 section 5's absent-guard case and the Corroboration acceptance.
The lodging capture assertion now reflects increment 4's supported plan 3
capture while retaining the run 1 sink refusal.

## Verification for this increment

Final checks:

| Command | Result |
|---|---|
| `cargo test -p rustev-pkg-lodging -p rustev-core -p rustev-runtime -p rustev-eval --locked --features rustev-core/parameterized,rustev-eval/parameterized` | Pass |
| `cargo test -p rustev-cli --locked` | Pass, including existing CLI goldens |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | Pass |
| `cargo fmt --all --check` | Pass |
| `git diff --check` and changed-file audit | Pass |

The requested single-worker filter reports every negative control detected:

```text
DETECTED spec 036: a protected field patched
DETECTED spec 036: a skipped value treated as empty
DETECTED spec 036: a retry on denied
DETECTED spec 036: a truncation without notice
DETECTED spec 036: tier order reversed
DETECTED spec 036: an uncaptured page dispatched in replay
seeds: 6/6 detected (1 worker(s))
```

The replay seed now compiles: `drop(work)` ends the dispatch future's borrow
before moving the pending request identity. The filter ran once with
`SEEDS_JOBS=1`; `spec 036` is registered in `OWN_RUNS`, not `REQUIRED`.
This run preceded the final corroboration and absent-input guard corrections;
it was not repeated.

No new published source files were added, so the archive allowlist needs no
change. Existing tracked goldens remain unchanged. Spec and derived files
remain unchanged; this report does not claim governance closure.
