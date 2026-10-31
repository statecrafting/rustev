# Increment 1 negative-case coverage

This table records coverage, not a conformance verdict. Socket tests remain enabled.
The optional runtime observer has byte-equivalence and panic-containment tests in
`crates/rustev-runtime/tests/progress.rs`.

| 042 section 5 case | Coverage or remaining dependency |
|---|---|
| Definition, fallback or endpoint in a decision | `bounded_retention_never_evicts_and_unknown_fields_cannot_change_authority` |
| Unregistered plan | Same test |
| PlanId mismatch | `registration_is_immutable_and_capacity_bounded` |
| Unknown signing key, tampered bundle, unsigned require-signed | `ed25519_signature_positive_tampered_invalid_and_unknown` |
| Profile-dependent signing | Excluded with spec 034 |
| Another signer for the same PlanId | `ed25519_signature_positive_tampered_invalid_and_unknown` |
| Registration without admin group | `http_routes_enforce_group_headers_declared_and_chunked_body_bounds` |
| Missing principal | In-memory HTTP test and strict principal tests |
| Both/neither snapshot/context | `malformed_snapshot_context_and_compiler_refusals_are_typed` |
| Escaping reference or symlink | `references_never_read_outside_configured_roots` |
| Unknown-scope lookup/cancel | Exact-decision and malformed-request tests |
| Clear TCP or TCP bearer, unsafe socket mode | `configuration_rejects_clear_tcp_bearer_and_unsafe_socket_modes` |
| Declared oversized body | Socket and in-memory HTTP tests |
| Admission queue full, queue deadline | `admission_overload_and_queue_deadline_do_not_create_retained_decisions` |
| Blocking pool queue full | `offline::tests::blocking_pool_full_and_queue_timeout_are_bounded_refusals` |
| Identical retry, conflicting retry | `exact_decision_is_retained_replayed_and_scope_isolated` |
| Retention full | Bounded-retention test |
| Disconnect | `disconnect_does_not_cancel_or_dispatch_twice` |
| Unconfirmed backend cancellation | `cancellation_never_promotes_unconfirmed_remote_work_to_stopped`; queued/running state and queued cancellation: `lookup_and_cancel_distinguish_queued_running_and_finished` |
| Sink fail-decision rejection | `delivery_failure_is_a_retained_decision_and_replays_500` |
| Remote sink timeout/conflict/wrong digest | `standalone_remote_sink_describes_delivers_and_rejects_bad_receipts`; sink unit tests check digest, durability, conflict and transport uncertainty |
| Remote context queue full | Handler propagation tested by `context_extra_fields_and_failed_fetch_never_dispatch_a_decision`; remote queue implementation remains in remote HTTP binding |
| Late context response | `conformance::callback::context_client_discards_a_late_answer_and_cancels_the_attempt` |
| Unrequested context field | Context negative test |
| Third-party endpoint, unknown describe version | Remote sink startup rejects unknown describe versions; remote context: `contexts::tests::described_sources_fetch_and_undescribed_ones_are_unavailable` and `rustev-remote-http` `construction_refuses_an_undescribed_source`; readiness requires host-confirmed describe completion |
| Invalidation gap/erasure | `pushed_invalidations_ack_in_order_and_report_gaps_and_erasures` (push: baseline, replay skipped, gap counted and degraded, erasure while retained counted); poll: `contexts::tests::a_polled_source_sets_its_baseline_applies_items_and_degrades_on_failure` |
| Stalled stream | `stalled_decision_stream_retains_final_and_counts_actual_step_drops`; live steps and disconnect: `http_stream_emits_admission_and_steps_before_final_and_disconnect_keeps_work` |
| HTTP/gRPC differential | `grpc_differential` covers success, refusals, retained streaming and evidence failure/lookup over HTTP/2 in memory and loopback; `grpc::tests` checks all 31 refusal mappings. Shared family conformance remains separate. |
| Explain unavailable | No longer reachable: spec 040 (D2) is implemented and `POST /v1/explain` is served; `grpc_differential::every_explain_golden_through_both_transports`, `grpc_differential::explain_checks_scope_and_resolves_references_under_roots` |
| Shutdown unflushed records | `immediate_shutdown_reports_every_pending_record`, `queued_sink_policies_preserve_tickets_drops_and_shutdown_accounting` |
| Forbidden dependency families | `make boundaries` |

## Spec 010 section 5 acceptance map

Paths below are repository-relative. Each serving behavior row names an offline
handler test and its control. These tests run with `cargo test -p rustev-serve
--locked`; none needs another integration crate or a live service. The existing
HTTP disconnect test uses an in-memory connection, so it also runs without socket
bind permission.

| 010 section 5 case | Applicability and exact test (file, test name) | Negative case and control |
|---|---|---|
| A crate outside `integrations/` depends on Aicortex, Rahi or an HTTP stack | Shared boundary: `tools/rustev-boundaries/src/lib.rs`, `tests::service_families_are_integration_only_and_transitively_impure` | Synthetic forbidden family edges outside integrations fail; the same edges under integrations pass. `make boundaries` checks the actual graph. |
| Contract or core gains an async executor through an integration helper | Shared boundary: `tools/rustev-boundaries/src/lib.rs`, `tests::a_transitive_executor_under_core_fails_with_its_path`; control: `tests::a_dev_only_executor_under_core_passes` | The helper-to-Tokio graph fails for both contract and core with the path; a dev-only edge passes. |
| Aicortex returns a claim without a stable revision where the plan requires freshness | Not a serving seam: owned by `rustev-aicortex` (3.5). | No serving coverage claimed. |
| A judgment is sent as an independently true Aicortex observation | Not a serving seam: owned by `rustev-aicortex` (3.5). | No serving coverage claimed. |
| Rahi times out after request bytes were sent | Not a serving seam: owned by `rustev-rahi` (3.6). | No serving coverage claimed. |
| Rahi reports the same decision id with a different digest | Not a serving seam: owned by `rustev-rahi` (3.6). | No serving coverage claimed. |
| A serving request names an unregistered plan or supplies a new fallback | `integrations/rustev-serve/tests/handlers.rs`, `unregistered_plan_and_fallback_are_refused_before_backend_dispatch` | Both requests produce typed refusals with zero backend dispatches; the registered plan with an unchanged request dispatches all three expected attempts and succeeds. |
| The client disconnects after serving dispatch | `integrations/rustev-serve/tests/handlers.rs`, `disconnect_preserves_uncertain_remote_charge_and_delivery_record`; transport coverage: `http_stream_emits_admission_and_steps_before_final_and_disconnect_keeps_work`; sink-in-flight coverage: `disconnect_does_not_cancel_or_dispatch_twice` | Dropping the waiter leaves the decision running. An unanswered backend reaches the original deadline with `PossiblyContinuing` and `Charge::Unknown` in the delivered record, without a cancelled response. In-progress and completed resubmissions do not dispatch again. Control: `unregistered_plan_and_fallback_are_refused_before_backend_dispatch` records answered attempts as `Finished` with observed charge; the HTTP disconnect test also allows backend completion after disconnect. |
| A serving response contains a resolved judgment | `integrations/rustev-serve/tests/handlers.rs`, `resolved_response_preserves_proposal_and_rejects_authorization_outcome` | HTTP success carries a typed `Propose` for the registered action. Replacing that outcome with a grant makes the response invalid. This checks the wire contract, not host authorization or execution. |
| Any external binding lacks the section 3.4 record | Governance prerequisite for every adapter, not runtime behavior under 3.7. Spec 010's serving record has owner approval A-47. | No runtime test can confer or simulate owner approval. A missing or refusing record prohibits implementation; the approved all-pass record permits implementation only. |

The shared boundary tests run with `cargo test -p rustev-boundaries --locked`;
`make boundaries` alone checks the live graph, not those synthetic negative
fixtures. The governance prerequisite and other adapters' rows are not claimed
as serving behavior tests or as completion of all spec 010 acceptance.

`sigstore_signed_plan_registers_offline_and_tampering_is_refused` registers a
synthetic Sigstore-signed Rustev plan, checks its signer, rejects changed bundle
content and rejects a changed inclusion proof. No trust or log fetch occurs.
`graceful_http_shutdown_delivers_final_before_closing_connection` verifies the
HTTP final event survives normal shutdown and the delivery is accounted.
