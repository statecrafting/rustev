# Structured extraction: acceptance coverage

Synthetic fixtures only. `definition.json` is the extraction example, not the
package reference definition. `pattern-a.json` uses the same presence-guarded
availability filter with the externally supplied window. No live provider runs.

Test paths below are relative to the workspace. `C` means
`crates/rustev-core/tests/`; `L` means this directory's `mod.rs`.

| Spec 038 Acceptance item | Tests or remaining dependency |
|---|---|
| 1. Model past window drops, caller past window refuses, extracted past window drops | L: `pattern_a_model_past_is_dropped_for_host`, `pattern_a_caller_past_is_refused_with_remediation`, `pattern_a_future_window_is_used_as_supplied`, `pattern_b_past_window_is_dropped_and_continues_undated`. Checks parties, remediation, causes, origins and derivations. |
| 2. Per-item vocabulary rejection, all-invalid absence, no coercion | L: `pattern_b_invalid_feature_is_rejected_per_item`, `pattern_b_party_size_is_not_clamped`, `pattern_b_area_is_not_truncated`; C/extraction_validate.rs: `lists_follow_all_five_ordered_checks`, `no_coercion_and_all_scalar_absence_reasons`, `records_fail_in_declared_order_and_name_second_subfield`, `record_level_faults_precede_field_faults_and_check_typed_constructors`. Section 5's other compile/evaluation refusals are covered in C/extraction_compile.rs and C/extraction_evaluate.rs. L: `pattern_b_blocked_untrusted_text_runs_only_exact_fallback` covers blocked untrusted text with the implemented 035 seam. |
| 3. Fallback origin, derivation and determinism | L: `pattern_b_backend_failure_is_filled_by_exact_fallback` compares repeated canonical judgments and evidence; C/extraction_fallback.rs: `derivation_tracks_all_reached_reads_in_sorted_unique_order`, `repeated_evaluation_has_identical_canonical_bytes`, `fill_modes_preserve_valid_model_values_and_distinguish_request_failure`. C/extraction_evaluate.rs: `screened_mixed_fallback_survives_acceptance_to_judgment` checks mixed derivation, both input origins and model producer metadata through acceptance to the judgment. `screening_resolves_before_extraction_and_never_dispatches_withheld_text` and runtime `tests/extraction.rs` cover pass, block, unavailability and readable exact fallback. No wasm test harness was found; native byte identity is tested, wasm parity is unverified. |
| 4. Complete distributions, calibration requirement, confidence never mass | L: `pattern_b_complete_logprobs_produce_a_distribution`; C/extraction_validate.rs: `distributions_use_declared_order_and_never_select_the_value`, `provider_report_and_fallback_class_are_preserved`; C/extraction_compile.rs: `acceptance_mass_calibration_binding_and_uncalibrated_notice`, `extraction_threshold_requires_option_logprobs`; C/extraction_evaluate.rs: `calibrated_mass_threshold_uses_bound_artifact`. |
| 5. Model-value gate refusal goldens, accepted reads/checks, controls refused | C/extraction_compile.rs: `model_gate_exact_policy_proposal_and_presence_reads`, `protected_constraint_and_model_projections_are_gated`, `accepted_paths_feed_exact_consumers_and_model_projections`, `accepted_value_cannot_control_selection_weights`. Eleven `extraction-gate-*.json` goldens pin exact, policy condition, proposal, protected check, extract/classify/generate projection and accepted control refusals. L Pattern A proves the 033 check accepts the model input. |
| 6. Needs input and declared question selection | L: `insufficient_requests_the_declared_window_question`, `extraction_proposes_only_a_declared_question`; C/extraction_evaluate.rs: `proposed_questions_use_allowed_order_declared_text_and_selected_continuation`, `proposed_questions_require_acceptance_of_a_matching_literal_enum_list`, `handler_question_selection_uses_the_same_accepted_values`. |
| 7. Producers, invalid producer and future timestamp | L Pattern A checks input evidence and lineage; C/extraction_contract.rs: `producer_refused_for_non_model_entries_before_evaluation`; C/extraction_evaluate.rs: `future_producer_timestamp_is_invalid_input_with_existing_precedence`, `input_acceptance_preserves_producer_and_supplied_values`. |
| 8. Origin/extraction metrics | `crates/rustev-eval/tests/origin_041.rs`: `extraction_accuracy_and_absence_compare_retained_values_with_gold` computes spec 041's `origin/extraction` accuracy and absence rates from C/golden/extraction.evidence.json. |
| 9. Existing golden and reference PlanId bytes | C/reference_plans.rs: `goldens_pin_both_definitions_plans_and_plan_ids`; C/extraction_compile.rs: `extraction_plan_identity_and_reload`, `acceptance_example_golden_and_member_identity`; packages/rustev-pkg-lodging/tests/documents.rs: `definition_matches_the_existing_core_golden_byte_for_byte`. Existing goldens are unchanged; only new refusal goldens are added. Full repository verification is not claimed by these three package suites. |
| 10. Code, declared verification, governance gate | `make code`, `spec-spine verify 038` (spec 038's Verification section, in `make verify` and CI's `integrations-base` group) and `make gate`. |

Decisions within the specification:

- Question lists retain their existing JSON; the object form is additive.
- Proposed ids are consumed only through an accepted literal enum list, ordered by `allowed`; absent or empty proposals use all allowed ids.
- Question text comes from declared templates; continuation identity includes selected ids.
- Future producer time is invalid input, preserving conflict precedence and per-entry rejection evidence.
- A dropped input can bypass eager dependency failure only for a filter with a leading decisive absence guard; unguarded reads still propagate missing evidence.
- Native fallback identity is checked because the repository has no wasm test harness.
