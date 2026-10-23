# Spec 030 offline coverage

Cases below preserve section 5 case text verbatim. Test paths are relative to
this directory unless fully qualified. These are deterministic synthetic and
loopback assertions, not model quality, live access or residency qualification.

## Section 5

| Case | Assertion coverage |
|---|---|
| Constructed without a `ModelPolicy` | `src/adapter.rs` constructor compile-fail doctest (`cargo test -p rustev-bedrock --locked --doc`). |
| Policy allows `eu.*` models and denies `wire: responses`; binding is `eu.` on `responses` | `contract.rs::exact_policy_negative_cases_name_the_admission_failure`. |
| Policy allows only `profile_kind: [none, geo]`; binding is `global.` | `contract.rs::exact_policy_negative_cases_name_the_admission_failure`. |
| Policy allows region `eu-*`; binding region `us-east-1` | `contract.rs::exact_policy_negative_cases_name_the_admission_failure` (region selector rejection). |
| `replace_policy` with a policy that no longer admits the binding | `loopback.rs::policy_replacement_and_precancel_never_obtain_credentials_or_send`. |
| Binding declares `inference_profile: none` for a `us.` id | `contract.rs::residency_and_profile_mismatches_refuse`. |
| Declared destinations include a region outside `allowed_destination_regions` | `contract.rs::residency_and_profile_mismatches_refuse`. |
| `global` profile under a policy without `allowed_destination_regions: any` | `contract.rs::residency_and_profile_mismatches_refuse`. |
| Region, profile, model or any parameter changes | `identity.rs::all_output_affecting_members_enter_identity`; `identity.rs::binding_changes_propagate_to_descriptor_and_plan_but_policy_does_not`. |
| Binding sets `operations: [rank]` or `guardrail` other than `none` before their specs land | `contract.rs::strict_documents_require_every_member_and_reject_unknowns`. |
| Responses binding with `provider_store: true` | `contract.rs::nested_documents_are_required_and_unknown_fields_refused`. |
| Plan step requires a distribution | `identity.rs::binding_changes_propagate_to_descriptor_and_plan_but_policy_does_not` (actual Bedrock descriptor). |
| Model returns `{"choice": "b", "confidence": 0.93}` | `contract.rs::documented_shapes_replay_and_extras_never_become_output`; `loopback.rs::responses_echo_stays_unknown_and_a_different_model_is_reported`. |
| Model answers a key that is not an option | `contract.rs::both_wires_all_operations_and_modes_replay_verbatim`; `identity.rs::binding_changes_propagate_to_descriptor_and_plan_but_policy_does_not` (core rejection). |
| No tool call, or two | `contract.rs::completion_statuses_and_tool_cardinality_on_both_wires`. |
| `stopReason: max_tokens` | `contract.rs::all_stops_missing_duplicate_answers_and_usage`; `contract.rs::completion_statuses_and_tool_cardinality_on_both_wires`. |
| 429 `ThrottlingException` | `loopback.rs::refusals_are_unknown_and_never_retried` (both wires, code, class and charge). |
| 429 `ModelNotReadyException` | `loopback.rs::refusals_are_unknown_and_never_retried`; `probe.rs::second_failure_stops_before_third_call_and_retains_type`. |
| 403 Marketplace agreement pending | `loopback.rs::refusals_are_unknown_and_never_retried`. |
| Response without `usage` | `contract.rs::documented_shapes_replay_and_extras_never_become_output` (both wires). |
| Responses `model` equals the requested id | `loopback.rs::responses_echo_stays_unknown_and_a_different_model_is_reported`. |
| Built without `aws-config`, with `AWS_ACCESS_KEY_ID` set and no provider | `loopback.rs::environment_credentials_cannot_construct_a_backend_without_a_provider`; source/dependency checks below. |
| Live smoke without `RUSTEV_BEDROCK_LIVE=1`, or reservation over the cap | `spend.rs::probe_without_live_flag_refuses_before_reading_files_or_credentials` (feature test); `probe.rs::exhausted_journal_refuses_without_a_request`. |
| `rustev-core` gains an `aws-*` dependency | `tools/rustev-boundaries/src/lib.rs::tests::cloud_sdk_family_obeys_both_rules`, via `cargo test -p rustev-boundaries --locked cloud_sdk_family_obeys_both_rules`; `make boundaries`. |

## Acceptance

| Acceptance bullet | Assertion coverage or explicit deferral |
|---|---|
| Documented-shape fixtures, and recorded fixtures once a smoke has run, with no network, for both wires: `classify`, `proposition` and `rubric` under each `structured_output` mode; every row of 3.10 including throttling and model-not-ready; every stop reason; missing usage (`unknown`); provider-reported extras retained and not supplied. | Offline: `contract.rs::both_wires_all_operations_and_modes_replay_verbatim`, `documented_shapes_replay_and_extras_never_become_output`, `completion_statuses_and_tool_cardinality_on_both_wires`, `all_stops_missing_duplicate_answers_and_usage`, `error_taxonomy_table`; `loopback.rs::refusals_are_unknown_and_never_retried`. Local taxonomy rows: `policy_replacement_and_precancel_never_obtain_credentials_or_send`, `credential_acquisition_is_inside_the_total_budget`, `connection_loss_after_request_is_interrupted_and_unknown`, `cancellation_after_write_is_unconfirmed`. Recorded live fixtures: BLOCKED on owner credentials, not claimed. |
| Fixtures for `rank`, `generate`, `extract` and screening on both wires are acceptance of C2, B5, B6 and B7 respectively (3.12.3). | DEFERRED to C2/B5/B6/B7; not acceptance of this change. Reserved-value refusals are asserted in `contract.rs::strict_documents_require_every_member_and_reject_unknowns`. |
| Allowlist refusals for model, region, wire and global-profile cases; deny wins; empty allow admits nothing; refusal names the rule; re-evaluation through `replace_policy` fails later attempts unsent. | `contract.rs::policy_denies_win_and_name_first_rule`, `exact_policy_negative_cases_name_the_admission_failure`; `loopback.rs::policy_replacement_and_precancel_never_obtain_credentials_or_send`. |
| Residency refusals for source region, profile kind, destination set, `global` without `any`, and prefix mismatch. | `contract.rs::residency_and_profile_mismatches_refuse`. |
| Identity changes when region, profile, destinations, residency policy, parameters, structured output, pricing or wire change; it does not change when the `ModelPolicy`, credentials or journal path change. A golden fixes one version 1 binding digest so later admitted values cannot move it. | `identity.rs::all_output_affecting_members_enter_identity`, `binding_changes_propagate_to_descriptor_and_plan_but_policy_does_not`, `version_one_binding_digest_is_fixed`. The journal is intentionally not an input to backend construction or compilation. |
| Existing reference-plan `PlanId` goldens are byte-identical. | `identity.rs::existing_reference_plan_bytes_and_ids_remain_identical`, `reference_builders_still_compile_to_original_goldens`. |
| Signing verified against the AWS Signature Version 4 test suite of Fact 11 (at least `get-vanilla`, `post-vanilla`, `get-vanilla-with-session-token`, `post-header-key-sort`, `get-utf8`, `get-vanilla-query-order-key-case`), plus a Bedrock-shaped request signed for service `bedrock`. | `signing.rs::published_aws_vectors`, `bedrock_request_signs_payload_service_region_token_and_fixed_time`. |
| Without the `aws-config` feature: the dependency graph contains no `aws-config`, a test with AWS environment variables set shows they are not read, and no `std::env` access exists outside the feature module and the smoke example. | `loopback.rs::environment_credentials_cannot_construct_a_backend_without_a_provider`, `environment_credentials_do_not_enable_the_probe_without_the_feature`; dependency and source checks below. Tests set only synthetic child-process credentials. |
| Cancellation before and after the first request byte; one total budget; credentials absent from every record and error detail. | `loopback.rs::policy_replacement_and_precancel_never_obtain_credentials_or_send`, `cancellation_after_write_is_unconfirmed`, `credential_acquisition_is_inside_the_total_budget`, `signed_loopback_success_records_identity_and_does_not_leak_credentials`, `refusals_are_unknown_and_never_retried`. Shared `Sink::deliver` checks every record in these tests for synthetic key, secret and token; refusal tests check error details and omit echoed credential-bearing provider messages. |
| `make boundaries` unit tests for the cloud SDK family (3.13). | `tools/rustev-boundaries/src/lib.rs::tests::cloud_sdk_family_obeys_both_rules`; command above. |
| Optional live smoke and probe behind `RUSTEV_BEDROCK_LIVE=1` and the R-29 testing journal, never in default CI. | Live execution and recorded fixtures BLOCKED on owner AWS credentials and excluded from this task. Offline core: `probe.rs::every_success_is_required_and_settled`, `second_failure_stops_before_third_call_and_retains_type`, `exhausted_journal_refuses_without_a_request`, `invalid_limits_refuse_before_dispatch`. Live flag: `spend.rs::probe_without_live_flag_refuses_before_reading_files_or_credentials`. |
| `make code`, declared verification, and `make gate` pass. | Whole-repository acceptance commands: `make code`, `make verify`, `make gate`. Not claimed by the six bounded crate checks in this task. Spec 030 declared verification registration remains deferred per its Implementation record and is outside territory. |

## Static verification

From the repository root, the following commands must produce no matches:

```sh
cargo tree -p rustev-bedrock --locked --offline --no-default-features -e normal,build | rg 'aws-config '
rg -n 'std::env' integrations/rustev-bedrock/src --glob '!aws_config_provider.rs'
```

`rg` exit 1 means no matches. Environment access in integration-test child
processes is deliberately outside the library source scan. No real AWS values
are read. The smoke example delegates to the feature module.

For the live-flag guard only (no live call), run:

```sh
cargo test -p rustev-bedrock --locked --features aws-config --test spend probe_without_live_flag_refuses_before_reading_files_or_credentials
```

Responses uses `status`, not Converse `stopReason`; the status tests cover
completed, incomplete, failed, cancelled, queued, in_progress and an unknown
value. Converse tests cover all nine documented stop reasons and an unknown
value. Transport, cancellation and policy failures are local events, so their
coverage uses loopback/control tests rather than invented provider fixtures.
