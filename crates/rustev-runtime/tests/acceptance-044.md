# Spec 044 acceptance mapping

Each Acceptance item and each section 5 row of
`specs/044-runtime-optimization-increment-2/spec.md`, with the test that
proves it. Test names are exact function names. Every test below runs in
`spec-spine verify 044`. Items still open are listed as open, never as
passed. SYNTHETIC data only: these tests establish mechanics, never quality
(R-04, R-37).

Abbreviations:

- SC: `crates/rustev-runtime/tests/shared_calls.rs`
- PE: `crates/rustev-runtime/tests/persistent.rs`
- DC: `crates/rustev-runtime/tests/decision_cache.rs`
- OP: `crates/rustev-runtime/tests/optimization.rs` (spec 017's tests and
  increment 1's)
- AQ, GE, EX: `crates/rustev-runtime/tests/acquisition.rs`,
  `generation.rs`, `extraction.rs`
- BM: `crates/rustev-runtime/tests/optimization_benchmark_044.rs`
- SL, ST, OD: the unit tests of `crates/rustev-runtime/src/shared.rs`,
  `src/store.rs` and `src/optimization.rs` (its `decision` module and
  `tests`)
- CX: `crates/rustev-core/tests/execution.rs`
- CU: the unit tests of `crates/rustev-core/src/execution.rs`
- CD: `crates/rustev-contract/tests/documents.rs`
- RI, RQ: `integrations/rustev-remote-http/tests/isolation.rs`,
  `batch_qualification.rs`
- AQ12: the stage 1 harness of the spec 012 adapter, `tests/batch_qualification.rs`
  in that adapter's integration crate
- BQ: the unit tests of `crates/rustev-eval/src/batch_qualification.rs`

## Acceptance

| Item | Proving test |
|---|---|
| 1. One deterministic test per section 5 row, with a manual clock and a manual executor, no wall-clock sleeps, and a spawn counter showing zero tasks spawned by shared calls | The section 5 table below. Every runtime test runs on Tokio's current-thread runtime and makes progress only by polling (`until`, `settle`), over a `ManualClock`. SC's sharing, handoff and last-waiter tests check `alive()`, the live task count, against the decisions the test started, and BM checks it in every shared-call cohort. Two rows remain open (rows 14 and 23). |
| 2. Last-waiter cancellation tests for each leave reason (cancel, deadline, drop) and each cancellation answer of spec 003 3.7.3 | Cancel: SC `the_last_waiter_cancels_polls_once_and_records_the_answer` with the answers `stopped`, `unconfirmed` and `not_observed`. Deadline: SC `the_last_waiters_deadline_ends_it_as_deadline_exceeded`. Drop: SC `a_dropped_owner_hands_off_and_a_dropped_last_waiter_abandons` (no cancellation requested, the reservation abandoned as unknown liability). The slot: SL `the_last_waiter_raises_the_signal_and_polls_once`, `a_dropped_waiter_hands_off_and_the_last_drops_the_attempt_unsignalled`, `concurrent_abandons_leave_exactly_one_last_waiter` |
| 3. Cross-tenant and cross-profile isolation negative tests at every tier | Tenants: SC `two_tenants_never_share`; OP `memory_cache_hits_expire_and_never_cross_scope` (request memory tier); PE `another_tenant_never_hits` (persistent tier); DC `two_tenants_never_share_an_entry` (decision memory tier); PE `a_restarted_runtime_serves_a_stored_decision` (decision persistent tier, its last part). Profiles: PE `another_profile_never_hits_at_any_tier` (shared calls, request memory and persistent tiers, decision memory and persistent tiers, with each profile still hitting its own); OD `the_profile_is_a_key_member`, `the_decision_key_follows_its_schema` |
| 4. Refresh-on-hit tests with a changed volatile value, and with each refresh-failure policy | Changed value: DC `a_changed_volatile_input_refreshes_its_closure`; GE `a_generate_step_reading_a_volatile_input_is_refreshed`; AQ `a_volatile_acquisition_is_refreshed_by_dispatch`. `unresolved`: DC `a_failed_refresh_is_supplied_as_it_is`. `serve_cached_within_freshness`: DC `a_missing_volatile_input_is_served_from_the_entry_within_its_max_age`, `a_stored_entry_past_its_max_age_is_stale_evidence`, `a_failed_refreshed_request_is_supplied_as_it_is`, `a_captured_decision_is_never_substituted`; AQ `a_failed_refreshed_acquisition_is_supplied_as_it_is`; PE `a_restarted_runtime_substitutes_a_stored_volatile_entry`. `recompute`: **open** (increment 5, part d2, pending an owner decision); CX `decision_cache_members_are_validated_and_unrun_parts_refused` shows it refused as not available in this build |
| 5. Persistence across a process restart, modelled as a new runtime over the same test store, including generation advance between the two | PE `a_restarted_runtime_hits_the_same_store` (a generation advanced before a third runtime makes every entry a miss with `generation_advanced`), `a_restarted_runtime_serves_a_stored_decision`, `a_restarted_runtime_substitutes_a_stored_volatile_entry`, `a_restarted_runtime_serves_stored_generated_text` |
| 6. Differential tests: shared, persistent-hit and decision-cache-hit execution against independent execution for outputs, failures, cancellation, deadlines, fallback and core evidence | Shared: SC `shared_execution_ends_as_independent_execution_does` (outputs, a permanent failure and a fallback, for the leader and the joiner: judgment, core evidence and request results, and the leader's transitions); cancellation and deadlines: SC `the_dispatching_waiter_cancels_and_hands_off_to_the_joiner`, `the_last_waiter_cancels_polls_once_and_records_the_answer`, `the_last_waiters_deadline_ends_it_as_deadline_exceeded` (each ends `cancelled` or `deadline_exceeded` with the records spec 003 gives an independent decision). Persistent hit: PE `a_restarted_runtime_hits_the_same_store` (judgment and core evidence against independent execution over an empty store), `a_fallback_output_is_never_written_and_runs_again_after_a_restart` (a failed primary answered by its fallback, never written, runs again, and the decision equals independent execution). Decision-cache hit: the served tests of DC and PE compare judgment and core evidence with independent execution (`same_judgment`, `same_as_independent`), as AQ, GE and EX do for their step kinds (increment 5, part e); DC `a_failed_request_outside_the_closure_is_not_admitted`. A cache hit dispatches nothing for a reused request, so it has no cancellation or deadline of its own; a cancelled decision writes nothing (increment 4, part b) |
| 7. Golden refusals for every 3.9 compile and preparation code | Compile: CX `incomplete_shared_and_persistent_members_are_refused` (`shared_call_bounds`, `persistent_bounds`, `store_contract_incomplete`) and `decision_cache_members_are_validated_and_unrun_parts_refused` (`decision_cache_bounds`, `volatile_unknown_source`, `volatile_without_freshness`, `key_dimension_invalid`), each detail string exact and each against a passing neighbour. Preparation: PE `a_persistent_tier_needs_a_store_with_the_plans_contract` (`store_missing`, `store_contract_mismatch`). Construction: `batch_unqualified` is **open** (increment 8, blocked on spec 022) |
| 8. Stage 1 of 3.8 for the remote-http and spec 012 adapters; stage 2 only under the work order's live scope, recorded with spend in the implementation record | Stage 1: RQ and AQ12 `stage_1_passes_and_the_report_stays_unknown_without_stage_2` (all fifteen cases); the report: BQ `the_verdict_follows_both_stages`, `pairs_compare_by_the_declared_criterion`, `a_report_round_trips_and_its_verdict_is_checked`, `malformed_members_are_refused`. Stage 2: **open**, live scope only; no spend is recorded because none was made |
| 9. Benchmarks report cold and warm p50, p95 and p99 latency and throughput for shared calls, the persistent tier and the decision cache, under the declared admission and concurrency limits recorded beside them, without an acceptance threshold | BM `report_cold_and_warm_shared_call_measurements`, `report_cold_and_warm_persistent_tier_measurements`, `report_cold_and_warm_decision_cache_measurements`, run with `--ignored --nocapture`. Each prints one JSON line with its limits and figures, and asserts only dispatch counts, store calls, live tasks and judgment equivalence |
| 10. Existing 009, 012 and 017 tests unchanged; `make code`, declared verification and `make gate` pass | The spec 009, 012 and 017 tests are unchanged and run under those specs' declared verification; OP runs here too. `spec-spine verify 044`; `make code` and `make gate` in CI |
| 11. The decision-result cache is implemented on spec 032's `rustev.plan/3` and `rustev.run/3`, not before them | CX `decision_cache_plan_members_are_a_plan_3_pair`, `decision_cache_alone_carries_a_version_2_definition`; CD `decision_cache_members_round_trip_and_are_omitted_when_absent` (refused outside `rustev.run/3`) |

## Section 5 observable negative cases

| Row | Case | Proving test |
|---|---|---|
| 1 | The dispatching waiter cancels while a joiner remains | SC `the_dispatching_waiter_cancels_and_hands_off_to_the_joiner`; SL `a_leaving_owner_returns_at_once_and_hands_off_to_the_earliest_waiter` |
| 2 | The last waiter cancels | SC `the_last_waiter_cancels_polls_once_and_records_the_answer`; SL `the_last_waiter_raises_the_signal_and_polls_once` |
| 3 | The last waiter's deadline passes | SC `the_last_waiters_deadline_ends_it_as_deadline_exceeded` |
| 4 | The last waiter's future is dropped | SC `a_dropped_owner_hands_off_and_a_dropped_last_waiter_abandons` (second half); SL `a_dropped_waiter_hands_off_and_the_last_drops_the_attempt_unsignalled`, `concurrent_abandons_leave_exactly_one_last_waiter` |
| 5 | The charge owner's future is dropped while a joiner remains | SC `a_dropped_owner_hands_off_and_a_dropped_last_waiter_abandons` (first half) |
| 6 | A request arrives after the shared attempt completed | SC `a_late_request_does_not_join_a_completed_attempt`; SL `leaving_after_completion_takes_the_result`, `joining_needs_time_room_and_an_attempt_in_flight` |
| 7 | A joiner's deadline is later than the attempt's dispatched deadline | SC `a_joiner_needing_more_time_runs_independently` |
| 8 | Two tenants send identical requests and decisions | SC `two_tenants_never_share`; OP `memory_cache_hits_expire_and_never_cross_scope`; PE `another_tenant_never_hits`; DC `two_tenants_never_share_an_entry`; PE `a_restarted_runtime_serves_a_stored_decision` |
| 9 | Same tenant, different profile | PE `another_profile_never_hits_at_any_tier`; OD `the_profile_is_a_key_member`, `the_decision_key_follows_its_schema`; CU `the_decision_key_schema_orders_its_members` |
| 10 | A declared key dimension is missing | DC `a_missing_key_dimension_bypasses_or_refuses`; OD `the_decision_key_follows_its_schema` |
| 11 | A volatile input changes between two identical decisions | DC `a_changed_volatile_input_refreshes_its_closure`; CU `the_refresh_closure_follows_the_step_graph`; GE `a_generate_step_reading_a_volatile_input_is_refreshed`; AQ `a_volatile_acquisition_is_refreshed_by_dispatch` |
| 12 | Refresh fails under `serve_cached_within_freshness`, value inside its maximum age | DC `a_missing_volatile_input_is_served_from_the_entry_within_its_max_age`, `a_stored_entry_past_its_max_age_is_stale_evidence`; PE `a_restarted_runtime_substitutes_a_stored_volatile_entry` |
| 13 | A refreshed request or acquisition fails under `serve_cached_within_freshness` | DC `a_failed_refreshed_request_is_supplied_as_it_is`; AQ `a_failed_refreshed_acquisition_is_supplied_as_it_is` |
| 14 | Refresh fails under `recompute` | **Open**: increment 5, part d2, pending an owner decision on how abandoned attempts sit in the run record. The compiler refuses the policy (CX `decision_cache_members_are_validated_and_unrun_parts_refused`), so no plan reaches this row in this build |
| 15 | A stored request key differs from the new one outside the closure | DC `a_moved_request_key_outside_the_closure_is_dispatched_alone` |
| 16 | A persistent entry is corrupt (integrity digest mismatch) | PE `a_corrupt_entry_is_a_miss_and_deleted`, `failed_entry_checks_are_decision_misses`; ST `every_failed_check_is_a_miss_with_its_diagnostic` |
| 17 | A persistent entry is from the future, of an old schema, or older than the store generation | PE `future_unknown_old_and_expired_entries_are_kept_misses`, `failed_entry_checks_are_decision_misses`; ST `every_failed_check_is_a_miss_with_its_diagnostic`, `a_decision_entry_reads_back_and_checks_its_kind` |
| 18 | Invalidation advances the generation while a decision runs | PE `an_invalidation_during_the_decision_keeps_its_result_out`, `a_moved_generation_or_a_budget_refuses_the_write`; DC `an_invalidation_during_the_decision_keeps_it_out`; OD `invalidation_reaches_decision_entries` |
| 19 | Two processes store an entry for the same key at once | PE `a_conflicting_put_keeps_the_output`, `failed_entry_checks_are_decision_misses` (a conflicting decision write); ST `put_answers_name_their_diagnostics` |
| 20 | The store fails under `fail_closed` | PE `store_failures_follow_the_failure_policy`, `store_failures_decide_the_decision_outcome` |
| 21 | A runtime restarts with the same store | PE `a_restarted_runtime_hits_the_same_store`, `a_restarted_runtime_serves_a_stored_decision`, `a_restarted_runtime_serves_stored_generated_text` |
| 22 | A per-item capability with batching enabled | OP `a_per_item_capability_is_never_batched`; RI `a_per_item_capability_is_never_batched_by_the_client`, `the_server_refuses_a_multi_item_request_for_a_per_item_capability`, `a_shared_capability_still_batches` (the neighbour); RQ and AQ12's per-item isolation case |
| 23 | A production binding enables batching without a qualified report | **Open**: increment 8, externally blocked on spec 022 (A-49). No binding enables adapter-scoped batching in production (R-28) |
| 24 | No new member is used | CD `shared_and_persistent_members_round_trip_and_are_omitted_when_absent`, `decision_cache_members_round_trip_and_are_omitted_when_absent`; CX `plan_identity_follows_the_optimization_policy`; `crates/rustev-core/tests/identity.rs` and `reference_plans.rs` (the existing goldens); OP `disabled_optimization_adds_nothing_to_the_record` |

The stage 1 harnesses (RQ, AQ12) run over loopback on Tokio's clock with
bounded waits, as increment 6, part b recorded; they prove 3.8 item 1, and
row 22's deterministic tests are OP and RI.
