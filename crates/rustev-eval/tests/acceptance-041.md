# Spec 041 acceptance mapping

Each Acceptance item and each section 5 row of
`specs/041-ranking-and-rubric-metrics/spec.md`, with the test that proves
it. Test names are exact function names (Rust) or test titles (Node).
Every Rust test below runs in `spec-spine verify 041`; the Node tests run in
CI's `bindings` job (`make bindings`), which builds the addon they need.

Abbreviations:

- RM: `crates/rustev-eval/tests/ranking_metrics.rs` (fixture
  `crates/rustev-eval/tests/data/metrics/ranking.json`)
- DO: `crates/rustev-eval/tests/documents_041.rs`
- CF: `crates/rustev-eval/tests/config_041.rs`
- ME: `crates/rustev-eval/tests/measured_041.rs` (fixtures
  `crates/rustev-eval/tests/data/metrics/constraint.json` and
  `rubric.json`)
- AG: `crates/rustev-eval/tests/agreement_041.rs` (fixture
  `crates/rustev-eval/tests/data/metrics/agreement.json`)
- OR: `crates/rustev-eval/tests/origin_041.rs`
- RP: `crates/rustev-eval/tests/reports.rs`
- SE: `crates/rustev-eval/tests/selection.rs`
- CE: `crates/rustev-cli/tests/eval.rs`
- CU: `crates/rustev-cli/tests/usage.rs`
- NS: `integrations/rustev-node/test/scorer.test.mjs`

## Acceptance

| Item | Proving test |
|---|---|
| 1. Unit tests for each metric of 3.4 to 3.8 against hand-computed fixtures, including empty, unjudged, unlabeled, `R = 0`, short-list, type-mismatch and unknown-attribute cases | Ranking (3.4.3, 3.4.4): RM `hand_computed_ranking_measures`, `every_excluded_case_is_excluded_from_every_cutoff`, `ndcg_never_divides_by_a_zero_ideal`, `gains_are_exact_and_total` against `data/metrics/ranking.json`, each case's arithmetic written in its `arithmetic` member. Over a split: ME `ranking_metrics_aggregate_over_the_split`, `every_case_excluded_is_unknown_never_zero`. Constraint (3.4.5, 3.4.6): ME `section_5_constraint_rows`, `share_at_k_counts_items_and_means_cases` against `data/metrics/constraint.json`. Origin (3.5): OR, every test. Rubric (3.6, 3.7): ME `hand_computed_rubric_metrics` against `data/metrics/rubric.json` (`mean_level_index`, `share_at_least`, `mean`, unscored, invalid, missing document, empty split, undeclared level, model placement); RP `version_3_scores_views_and_reports_rubric_metrics` over the whole pipeline. Agreement (3.8.4): AG `qwk_by_hand`, `judges_and_human_labels_agree_by_hand` against `data/metrics/agreement.json`. Type mismatch: RM `gold_values_match_by_kind_only`. Every fixture case writes its arithmetic in its `arithmetic` member. |
| 2. `discounts.py --check` reproduces `discounts.json`; a Rust test checks the embedded table | `python3 -I crates/rustev-eval/tests/data/metrics/discounts.py --check`; RM `embedded_discounts_match_the_committed_table` |
| 3. A TypeScript scorer through the Node binding is byte-identical to a Rust scorer; throwing, late and queue-full scorers map as 3.9.1 states | CE `the_scorer_fixture_reproduces` pins the Rust half (inputs, views, scores, report under `crates/rustev-cli/tests/golden/scorer`). NS `a TypeScript scorer reproduces the Rust scorer byte for byte`, `a throwing or rejecting scorer is scorer_failed, never a score`, `a full queue is scorer_overloaded at once`, `a late scorer is scorer_deadline, aborted, and its settlement discarded and counted`, `a cancelled call is scorer_deadline, and its signal is aborted`, `an answer outside the output, or a reserved reason, is invalid_score`. Spec 027 is complete, so this item is no longer externally blocked; the Node half runs in CI's `bindings` job. |
| 4. A model judge over captured bundles of a judge plan reports only under `model/`, after every deterministic metric; changing its scores changes no deterministic metric | RP `judge_bundles_reproduce_into_scores` (the support-routing plan as judge, captured bundles, model derivation); RP `version_3_scores_views_and_reports_rubric_metrics` (placement, and the deterministic metrics' bytes unchanged when the judge's scores change); CE `eval_judge_writes_scores_from_judge_bundles` |
| 5. Judge agreement golden: judge versus human, two judges, `qwk` unknown | AG `judges_and_human_labels_agree_by_hand`, `one_level_on_both_sides_makes_qwk_unknown_and_exact_one`, `agreement_forms_follow_3_11_1`, `sides_on_different_scales_are_never_indexed` |
| 6. Spec 029 integration: reconciliation, bootstrap intervals, `not-case-decomposable` `qwk`, a selection gate over `ranking/<name>/ndcg@10` and `model/rubric/<name>` that qualifies, rejects and is unknown | RP `version_3_repeat_sets_reconcile_and_select` (exit codes 0, 11 and 12); RP `version_3_reports_ranking_and_constraint_metrics` (reconciliation of the 3.4 and origin metrics) |
| 7. Origin families tested once their owning spec's evidence is implemented | Every family is implemented: OR `screening_rates_per_screener_exclude_empty_values`, `corroboration_counts_contradicted_over_decided_outcomes`, `acquisition_tiers_count_where_each_decision_stopped`, `text_backstop_rates_count_slots_and_aggregate_origins`, `extraction_accuracy_and_absence_compare_retained_values_with_gold`, `a_plan_without_the_construct_makes_every_metric_unknown`, `every_family_is_available`; CF `section_5_configuration_refusals` accepts every family |
| 8. Every row of section 5 has a test | The table below |
| 9. Compatibility: existing eval, CLI and 029 goldens, plan and definition goldens with their `PlanId`s byte-identical | DO `version_1_keeps_its_bytes_and_identity`; CF `version_3_members_belong_to_version_3_only`; the whole `rustev-eval` and `rustev-cli` suites, whose goldens this spec does not change; `crates/rustev-core/tests/reference_plans.rs` and `identity.rs`; `rustev-surface`'s adapter test `version_2_requires_measures_and_tags_its_digest` (a version 1 adapter keeps its digests) |
| 10. The eight mutation seeds are each detected | `sh crates/rustev-eval/mutation/seeds.sh "spec 041"` (the eight `spec 041` seeds in `crates/rustev-eval/mutation/seeds.py`, run under their own filter as an `OWN_RUNS` category) |
| 11. `make code`, declared verification and `make gate` pass | `spec-spine verify 041`; `make code`; `make gate` |

## Section 5 observable negative cases

| Row | Case | Proving test |
|---|---|---|
| 1 | Top 5 holds 2 unjudged items, 1 relevant of 3 judged | RM `hand_computed_ranking_measures` (`ranking.json`, section 5 row 1) |
| 2 | Top 5 holds only unjudged items, `R = 2` | RM `hand_computed_ranking_measures` (row 2); RM `every_excluded_case_is_excluded_from_every_cutoff` |
| 3 | No gold, or no `relevance` | RM `hand_computed_ranking_measures` (row 3); ME `ranking_metrics_aggregate_over_the_split` |
| 4 | `R = 0` | RM `hand_computed_ranking_measures` (row 4); RM `ndcg_never_divides_by_a_zero_ideal` |
| 5 | Every case excluded | ME `every_case_excluded_is_unknown_never_zero` |
| 6 | A list of 3 under `precision@5` | RM `hand_computed_ranking_measures` (row 6); ME `ranking_metrics_aggregate_over_the_split` (`short@5`) |
| 7 | A ranking naming one id twice | RM `hand_computed_ranking_measures` (`refused`, row 7) |
| 8 | Parameter is a `Filtered` value | RM `hand_computed_ranking_measures` (`refused`, row 8) |
| 9 | `all_at_k`, one unknown attribute, no failure | ME `section_5_constraint_rows` |
| 10 | `all_at_k`, reference absent, no item failing | ME `section_5_constraint_rows` |
| 11 | `all_at_k`, one unknown attribute and one failure | ME `section_5_constraint_rows` |
| 12 | `all_at_k` over an empty list | ME `section_5_constraint_rows` |
| 13 | `path_prefix: 2` against a one-element path | ME `section_5_constraint_rows` |
| 14 | Extraction accuracy, `Integer` value against a string gold value | RM `gold_values_match_by_kind_only`; OR `extraction_accuracy_and_absence_compare_retained_values_with_gold` (`type_mismatch` counted) |
| 15 | Constraint `equals` against an `Integer` parameter and a string attribute | ME `section_5_constraint_rows`; ME `share_at_k_counts_items_and_means_cases` |
| 16 | `origin: ["text_backstop"]` before spec 037 | Not reachable in this build: spec 037 is complete and spec 038's evidence is on `main`, so every family is available (increment 6 part b). CF `section_5_configuration_refusals` and OR `every_family_is_available` prove no family is refused; the `metric-unavailable` code remains in `crates/rustev-eval/src/scoring.rs` for a family whose evidence is absent. |
| 17 | `measures` digest differs from the adapter's | RP `version_3_reports_ranking_and_constraint_metrics` (`EvalError::Adapter` before any case) |
| 18 | Scores document over another evaluation's views | RP `version_3_scores_views_and_reports_rubric_metrics`; CE `version_3_eval_writes_views_and_reads_scores` |
| 19 | Declared scorer with no scores document | RP `version_3_scores_views_and_reports_rubric_metrics` (`scores-missing`); AG `judges_and_human_labels_agree_by_hand` |
| 20 | Level index 7 on a 5-level scale | RP `version_3_scores_views_and_reports_rubric_metrics` (`invalid_score`, never clamped) |
| 21 | Judge bundle missing | RP `judge_bundles_reproduce_into_scores`; CE `eval_judge_writes_scores_from_judge_bundles` |
| 22 | Judge bundle subject differs from the view | RP `judge_bundles_reproduce_into_scores` |
| 23 | Model descriptor with `bounded` output | CF `section_5_configuration_refusals` (`judge-output-kind`) |
| 24 | Deterministic scorer with `reads_gold: false` | RP `version_3_scores_views_and_reports_rubric_metrics` (the scorer asserts its case view's `gold` is null); NS `gold is withheld unless the scorer declares reads_gold` |
| 25 | `eval judge` without `--config` | CU `eval_judge_requires_its_configuration` |
| 26 | `exact-derived` judgment under a model descriptor | RP `judge_bundles_reproduce_into_scores` (`judge-derivation-mismatch`) |
| 27 | A judge bundle of another plan | RP `judge_bundles_reproduce_into_scores` (`judge-plan-mismatch`, no scores) |
| 28 | Model scorer's metric without `model/` | CF `model_metrics_never_go_unprefixed` |
| 29 | `mean_level_index` over a bounded scorer | CF `section_5_configuration_refusals` |
| 30 | `share_at_least` naming an undeclared level | CF `section_5_configuration_refusals`; ME `hand_computed_rubric_metrics` (`rubric.json`, section 5 row 30) |
| 31 | Agreement with every case on one level | AG `one_level_on_both_sides_makes_qwk_unknown_and_exact_one` |
| 32 | A TypeScript scorer that throws | NS `a throwing or rejecting scorer is scorer_failed, never a score` |
| 33 | A TypeScript scorer whose queue is full | NS `a full queue is scorer_overloaded at once` |
| 34 | Gate over an opaque scorer's metric | RP `version_3_gates_refuse_opaque_scorers_and_other_measures`; SE `an_opaque_scorers_metric_is_unknown_and_unranked`; RP `version_3_repeat_sets_reconcile_and_select` (exit 12) |
| 35 | `--scores` with a version 2 configuration | CE `scores_usage_and_version_rules` (exit 2, empty stdout, nothing written) |
| 36 | Existing `rustev eval` and `rustev gate` on today's inputs | The existing `crates/rustev-cli/tests/eval.rs` goldens and `crates/rustev-eval/tests/reports.rs` gate tests, unchanged |
