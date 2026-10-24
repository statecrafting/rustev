# Explanation: `"why_outcome"`

| Side | Form | Decision | Plan | Capture |
| --- | --- | --- | --- | --- |
| a | record | `"explain-lodging"` | `"sha256:ecfbc309595d6e7b970e7b6deb1ba69078587e14b6f552b12ff5fc31e9ffcac5"` | `null` |

The recorded outcome is a proposal.

| Claim | Value | Derivation | Lineage | Cites |
| --- | --- | --- | --- | --- |
| outcome | `{"propose":{"action":"present_ranked_lodging","params":{"eligibility":{"filtered":{"counts":{},"eligible":["c-1","c-2","c-3","c-4","c-5","c-6","c-7","c-excluded"],"excluded":[]}},"ranking":{"ranking":{"entries":[{"candidate":"c-1","components":{"preference...` | `{"bound":"judgment","class":"mixed-derived"}` | `{"bound":"judgment","inputs":["fx.rates","inventory.candidates","traveler.claims","trip.budget","trip.dates","trip.party_size","trip.requires_step_free","trip.text"],"steps":["active_claims","eligible","missing_trip_fields","ranking","shortlist","suitabili...` | `["record:/core/recorded/judgment/derivation","record:/core/recorded/judgment/lineage","record:/core/recorded/judgment/outcome"]` |
| matched | `"rule:ranked"` | `null` | `null` | `["record:/core/recorded/judgment/trace/0"]` |
| rule_declaration | `{"id":"ranked","then":{"propose":{"action":"present_ranked_lodging","params":[{"name":"ranking","value":{"ref":"step:ranking"}},{"name":"shortlist","value":{"ref":"step:shortlist"}},{"name":"eligibility","value":{"ref":"step:eligible"}}]}},"when":"always"}` | `null` | `null` | `["plan:/definition/policy/rules/2"]` |
| adjustments | `["rule:ranked"]` | `null` | `null` | `["record:/core/recorded/judgment/trace"]` |
| notices | `[]` | `null` | `null` | `["record:/core/recorded/judgment/notices"]` |
| judgment_derivation | `"mixed-derived"` | `{"bound":"judgment","class":"mixed-derived"}` | `{"bound":"judgment","inputs":["fx.rates","inventory.candidates","traveler.claims","trip.budget","trip.dates","trip.party_size","trip.requires_step_free","trip.text"],"steps":["active_claims","eligible","missing_trip_fields","ranking","shortlist","suitabili...` | `["record:/core/recorded/judgment/derivation","record:/core/recorded/judgment/lineage"]` |
| judgment_lineage | `{"inputs":["fx.rates","inventory.candidates","traveler.claims","trip.budget","trip.dates","trip.party_size","trip.requires_step_free","trip.text"],"steps":["active_claims","eligible","missing_trip_fields","ranking","shortlist","suitability","supports_prefe...` | `{"bound":"judgment","class":"mixed-derived"}` | `{"bound":"exact","inputs":["fx.rates","inventory.candidates","traveler.claims","trip.budget","trip.dates","trip.party_size","trip.requires_step_free","trip.text"],"steps":["active_claims","eligible","missing_trip_fields","ranking","shortlist","suitability"...` | `["record:/core/recorded/judgment/derivation","record:/core/recorded/judgment/lineage"]` |
| termination | `"judged"` | `null` | `null` | `["record:/termination"]` |
| addressed_to | not available: not recorded (facet addressed_to) | `null` | `null` | `[]` |
| remediation | not available: not recorded (facet remediation) | `null` | `null` | `[]` |
| caused_by | not available: not recorded (facet caused_by) | `null` | `null` | `[]` |
