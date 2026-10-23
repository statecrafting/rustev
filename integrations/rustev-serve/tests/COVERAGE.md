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
| Late context response | Remote callback client accounting remains outside this increment |
| Unrequested context field | Context negative test |
| Third-party endpoint, unknown describe version | Remote sink startup rejects unknown describe versions; remote context construction remains outside this increment; readiness requires host-confirmed describe completion |
| Invalidation gap/erasure | Remote invalidation implementation remains outside this increment; readiness degradation reporting is tested |
| Stalled stream | `stalled_decision_stream_retains_final_and_counts_actual_step_drops`; live steps and disconnect: `http_stream_emits_admission_and_steps_before_final_and_disconnect_keeps_work` |
| HTTP/gRPC differential | Excluded gRPC/shared conformance increment |
| Explain unavailable | `in_memory_http1_and_http2_use_the_same_handlers` |
| Shutdown unflushed records | `immediate_shutdown_reports_every_pending_record`, `queued_sink_policies_preserve_tickets_drops_and_shutdown_accounting` |
| Forbidden dependency families | `make boundaries` |

010 section 5 serving rows: request injection/unregistered-plan refusal,
disconnect ownership and proposal preservation are covered by the handler tests.
The dependency rows are checked by `make boundaries`; Aicortex/Rahi rows belong
to those integrations. The binding-record approval is a governance prerequisite,
not a test that can confer owner approval.

`sigstore_signed_plan_registers_offline_and_tampering_is_refused` registers a
synthetic Sigstore-signed Rustev plan, checks its signer, rejects changed bundle
content and rejects a changed inclusion proof. No trust or log fetch occurs.
`graceful_http_shutdown_delivers_final_before_closing_connection` verifies the
HTTP final event survives normal shutdown and the delivery is accounted.
