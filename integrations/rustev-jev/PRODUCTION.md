# Spec 022 implementation status

This change implements schema-2 binding facts and synthetic mechanics. It does
not establish a verified direct API mapping, production eligibility, live smoke
results, qualification, or deployment approval.

`ProductionBinding` is a separate `rustev.jev-binding/2` document. Its canonical
identity includes use, the decision binding, mapping, endpoint, API version, TLS
facts, phase ceilings, connection policy, privacy terms and request options,
price source and rates, credential custody class, and explicit version tolerance.
Schema 1 is unchanged; its default canonical bytes and digest have a golden test.
Missing, invalid, conflicting and expired facts return `Unresolved`.

`JevBackend::new_production` refuses before credential lookup or dispatch. No
recorded, owner-authorized direct API exchanges or accepted Gateway pinning fact
are available in the repository. Supplying a string claiming such evidence does
not enable a mapping. Vendor documentation alone cannot enable Gateway pinning.

`JevBackend::new_mechanics` is a loopback-IP-only stage-1 harness, using a mandatory
persistent testing journal and a host credential callback at dispatch. Supply
only synthetic credentials. Its mapping is explicitly
`rustev-jev-direct-synthetic/1`, based on synthetic fixture shapes, not a claim
about the provider's direct API. The callback must return promptly. It runs only
after the journal reservation and request checks. The harness never retries,
follows redirects, reuses connections, or switches transport. Phase ceilings
are recorded individually for all seven spec 020 phases and passed directly to
the phase clock. Each positive ceiling is clipped to the one remaining attempt
budget when that phase starts. Reuse is explicitly `false`, with
`one-connection-per-attempt` as its isolation fact; reuse-on bindings are refused
until bounded pooling and the full spec 020 isolation key are implemented. TLS
facts describe the future production binding; the explicit mechanics destination is a local fixture peer.

A matching pin or explicitly tolerated version records the actual reported
identity. A different, absent or incorrectly shaped version refuses all outputs.
Provider confidence remains provider-reported. Cost is observed from a reported
charge, estimated from usage and the bound price table, or unknown. Requested
privacy posture is recorded on each exchange, including explicit unsupported
zero retention. No credential enters the binding or exchange record.

`check_production_plan` is the host's preparation check over primary and fallback
artifacts; missing or development bindings refuse. It does not change the core
compiler. `check_evidence` refuses evidence and calibration associations under
another binding identity. `Qualification::check` additionally requires dated
facts, authorization, report and spend references, and admitted data when
available; synthetic provenance needs an explicit limitation. It cannot accept
production qualification while the mapping remains unverified.

`Reconciliation::compare` compares a month's observed, estimated, liability and
outstanding totals with the same credential's billing record, preserving signed
differences without changing entries. The host retains the returned record and
provider invoice. `SpendJournal::reconcile_billing` settles a production liability
only from an explicit provider billing line for that reservation's month and
credential reference. Hosts must retain that line. A late mechanics response
makes a reconciliation offer but does not automatically settle liability.

The core does not expose a `screen` operation. This adapter declares no screening
capability. Spec 022 section 3.8.4 is conditional on that seam being delivered.

Live stage 2 and stage 3 are ignored tests with explicit reasons. Enabling them
requires owner-supplied access and credentials, recorded direct request and
response shapes, a verified mapping, data admission or the stated synthetic
limitation, and the shared USD 5 testing cap. Production privacy decisions,
production spend authority, and deployment approval remain separate owner acts.
Spec 022 declares its verification (`spec-spine verify 022`, part of
`make verify`) and records stages 2 and 3 as externally blocked.
