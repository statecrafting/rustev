# Spec 039 ranking acceptance

Increment 7 part a. Synthetic fixtures only. Test mappings are coverage claims,
not a claim that repository verification or every test passes. Paths below are
relative to the named crate. `core` = `crates/rustev-core`, `runtime` =
`crates/rustev-runtime`, `jev` = `integrations/rustev-jev`, `rules` =
`backends/rustev-backend-rules`, `contract` = `crates/rustev-contract`.

## Acceptance items

| Acceptance item | Crate, file, test |
|---|---|
| Full list, partial list, duplicates, unknown ids, outside-pool ids; origins at every position | core, `tests/ranking_value.rs`, `listwise_origin_fixtures_validate_before_selecting` |
| Backend failure under backfill_all and propagate; evidence failures | core, `tests/ranking_select.rs`, `only_backend_failures_allow_backfill_all`; `tests/ranking_evaluate.rs`, `failed_backend_backfills_but_missing_evidence_propagates`, `five_positions_and_stale_candidate_evidence` |
| Backfill exhausted, unresolved when needed and not needed | core, `tests/ranking_select.rs`, `no_backfill_shortfall_and_lazy_unresolved_backfill` |
| Pointwise | core, `tests/ranking_value.rs`, `pointwise_partial_ties_scope_and_all_failure_order` |
| Listwise with and without scores | core, `tests/ranking_value.rs`, `listwise_drops_and_scores_keep_returned_order`, `listwise_origin_fixtures_validate_before_selecting`, `listwise_validates_the_whole_output_and_preserves_first_failure` |
| Permutation averaging, Borda ties, id_ascending and order_of | core, `tests/ranking_value.rs`, `borda_uses_retained_positions_never_combines_scores_and_is_deterministic` |
| Pairwise pools 0 to 12, both schedules, cap and worst-case bounds | core, `tests/ranking_schedules.rs`, `pools_zero_through_twelve_stay_bounded_and_replay_identically`, `merge_caps_never_expose_an_extra_presentation`, `merge_bound_matches_exhaustive_winner_paths` |
| Injected cycle, wins then tie-break, counted | core, `tests/ranking_schedules.rs`, `round_robin_order_bias_and_cycles`; `tests/ranking_value.rs`, `pairwise_cycle_inconsistency_and_invalid_answers` |
| Identical schedules/results across runs and replay of supplied answers | core, `tests/ranking_schedules.rs`, `pools_zero_through_twelve_stay_bounded_and_replay_identically`, `merge_sort_five_exact_sequence_and_determinism` |
| Captured bundle replay | jev, `tests/runtime.rs`, `pairwise_capture_replays_both_schedules_without_backend_calls`; eval, `tests/ranking_replay.rs`, `rank_capture_replays_exact_evidence_without_dispatch` (`crates/rustev-eval/src/replay.rs::target_artifact` accepts ranking bindings) |
| Both-order disagreements counted | core, `tests/ranking_schedules.rs`, `numeric_comparison_order_and_disagreement_tie_breaks`; `tests/ranking_value.rs`, `pairwise_cycle_inconsistency_and_invalid_answers` |
| rustev.permute/1 known-answer vectors | core, `tests/permute_vectors.rs`, `independent_python_known_answers` |
| Permutations independent of backend | core, `tests/ranking_evaluate.rs`, `permutations_change_with_seed_and_survive_backend_swap` |
| Every 3.12 refusal row with passing neighbour | core, `tests/ranking_compile.rs`, `refusal_rows_have_passing_neighbours`; detail cases in `structural_refusals_are_phase_b_before_unknown_operators`, `type_and_exact_source_refusals_precede_phase_f`, `capability_shortfalls_are_phase_f_before_budget`, `phase_g_bounds_and_visible_truncation`, `wire_controls_keep_category_11_and_kind_fallback_is_refused` |
| 3.9.1 compile-fail conversions | core, `src/ranking/mod.rs` doctests: ModelRanking and Selection, each direction against all six other kinds (24 cases) |
| Derivation and lineage per position and step | core, `tests/ranking_select.rs`, `backfill_exclusions_positions_derivation_lineage_and_determinism`, `selection_unions_producers_from_both_sources`; `tests/ranking_value.rs`, `ranking_preserves_input_producers_on_success_and_failure`; `tests/ranking_evaluate.rs`, `five_positions_and_stale_candidate_evidence`, `selection_is_a_mixed_derived_proposal_parameter` |
| Rules pointwise tasks and refusals | rules, `tests/ranking.rs`, `pointwise_decimal_contributions_and_descriptor`, `unsupported_modes_and_invalid_candidate_envelopes_are_capability_failures`, `plan_check_validates_presented_item_fields_and_task_identity` |
| Bedrock fixtures, both wires, all modes | bedrock (`integrations/rustev-bedrock`), `tests/ranking.rs`, `documented_fixtures_map_on_both_wires_for_the_three_modes`, `fixtures_answer_through_the_signed_adapter_on_loopback_only`, `answers_are_supplied_as_received_or_malformed_never_repaired` |
| Rules/Bedrock swap and comparable evaluation reports | deferred by A-49 to the Bedrock increment's part b |
| Compatibility goldens | core, `tests/reference_plans.rs`, `goldens_pin_both_definitions_plans_and_plan_ids`; contract, `tests/ranking_wire.rs`, `existing_descriptor_goldens_keep_bytes_and_descriptor_ids`; `tests/ranking_results.rs`, `existing_goldens_keep_bytes_and_identities`; rules, `tests/identity.rs`, `an_unchanged_program_reproduces_every_identity` |
| make code, declared verification, make gate | Not claimed here. This brief requests four package suites, core doctests, workspace clippy and fmt; adding 039 to Makefile verification is outside this increment's authorized paths. |

## Section 5 observable negative cases

| Row | Crate, file, test |
|---|---|
| Rank in definition 1 | core, `tests/ranking_compile.rs`, `structural_refusals_are_phase_b_before_unknown_operators` |
| Policy reads rank | core, `tests/ranking_compile.rs`, `type_and_exact_source_refusals_precede_phase_f` |
| mass_at_least over selection | core, `tests/ranking_compile.rs`, `refusal_rows_have_passing_neighbours` |
| Model-derived shortlist backfill | core, `tests/ranking_compile.rs`, `model_derived_shortlist_backfill_refused_with_exact_neighbour` |
| Listwise bound 60, backend limit 50 | core, `tests/ranking_compile.rs`, `refusal_rows_have_passing_neighbours` |
| Round robin bound 10, cap 44 | core, `tests/ranking_compile.rs`, `refusal_rows_have_passing_neighbours` (45 passes) |
| Shuffle on pointwise | core, `tests/ranking_compile.rs`, `structural_refusals_are_phase_b_before_unknown_operators` |
| b,b,zz,x,a drops duplicate, unknown, outside pool | core, `tests/ranking_value.rs`, `listwise_drops_and_scores_keep_returned_order`, `listwise_origin_fixtures_validate_before_selecting` |
| Listwise scores length mismatch | core, `tests/ranking_value.rs`, `listwise_validates_the_whole_output_and_preserves_first_failure` |
| One pointwise timeout | core, `tests/ranking_value.rs`, `pointwise_partial_ties_scope_and_all_failure_order` |
| Every pointwise candidate fails | core, `tests/ranking_value.rs`, `pointwise_partial_ties_scope_and_all_failure_order` |
| Round robin a>b, b>c, c>a | core, `tests/ranking_schedules.rs`, `round_robin_order_bias_and_cycles` |
| Round robin pool 3, orders 2, six distinct keys counted | core, `tests/ranking_schedules.rs`, `round_robin_order_bias_and_cycles` |
| Merge comparison failure | core, `tests/ranking_value.rs`, `merge_sort_outputs_and_failures_follow_schedule_not_map_order` |
| k=5, three model positions then two backfill | core, `tests/ranking_evaluate.rs`, `five_positions_and_stale_candidate_evidence` |
| Outside-pool backfill skipped and counted | core, `tests/ranking_select.rs`, `backfill_exclusions_positions_derivation_lineage_and_determinism` |
| Backend failure, backfill_all, fallback notice, mixed class | core, `tests/ranking_select.rs`, `only_backend_failures_allow_backfill_all`; `tests/ranking_evaluate.rs`, `failed_backend_backfills_but_missing_evidence_propagates` |
| Backend failure, propagate | core, `tests/ranking_select.rs`, `only_backend_failures_allow_backfill_all`; jev, `tests/runtime.rs`, `offline_jev_mapping_composes_selection_and_propagates_failures` |
| Stale candidate evidence | core, `tests/ranking_evaluate.rs`, `five_positions_and_stale_candidate_evidence` |
| Backfill exhausted | core, `tests/ranking_select.rs`, `no_backfill_shortfall_and_lazy_unresolved_backfill` |
| Seed changes identity and presentations | core, `tests/ranking_evaluate.rs`, `permutations_change_with_seed_and_survive_backend_swap` |
| Backend swap preserves presentations, changes binding identity | core, `tests/ranking_evaluate.rs`, `permutations_change_with_seed_and_survive_backend_swap` |
| Rules listwise rank request refused | rules, `tests/ranking.rs`, `unsupported_modes_and_invalid_candidate_envelopes_are_capability_failures` |
| Bedrock rank without structured output | bedrock, `tests/ranking.rs`, `construction_refuses_rank_without_structured_output_and_invalid_rank_bindings` |
| Existing goldens recompiled unchanged | Compatibility mappings above; existing package suites retain their golden assertions. Full corpus verification is not claimed by this bounded test increment. |

## Jev composition and remaining A-49 scope

| Requirement | Crate, file, test or disposition |
|---|---|
| 3.16: Jev proposition/rubric fan-out plus weighted_rank feeds select, including failure detection | jev, `tests/runtime.rs`, `jev_composed_ranking_selects_and_detects_total_backend_failure` (loopback); `offline_jev_mapping_composes_selection_and_propagates_failures` (actual Jev wire mapping, no sockets) |
| No rank operation, no ranking descriptor member, no synthesized scores | jev, `tests/adapter.rs`, `the_descriptor_declares_three_distribution_operations_and_no_rank`; offline composition above asserts distribution outputs |
| 3.18.1 Bedrock bindings without rank | bedrock, `tests/ranking.rs`, `bindings_without_rank_keep_their_bytes_and_identities`; `tests/identity.rs` |
| Paid live Bedrock smoke | deferred by A-49 |

The captured pairwise replay regression (`crates/rustev-eval/tests/ranking_replay.rs`)
is active and passes with the `Rank` arm in `replay.rs`'s target lookup.

Verification: spec 039 declares the commands (`spec-spine verify 039`), and CI runs them.
