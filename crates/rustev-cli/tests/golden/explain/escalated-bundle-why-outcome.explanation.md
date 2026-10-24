# Explanation: `"why_outcome"`

| Side | Form | Decision | Plan | Capture |
| --- | --- | --- | --- | --- |
| a | bundle | `"explain-support"` | `"sha256:011d1ba8a7f4a78f000393c6620b8bc218853b2fc05941cd2b6da6e44afaf47d"` | `"complete"` |

The recorded outcome is an escalation.

| Claim | Value | Derivation | Lineage | Cites |
| --- | --- | --- | --- | --- |
| outcome | `{"escalate":{"reason":"ambiguous-topic"}}` | `{"bound":"judgment","class":"mixed-derived"}` | `{"bound":"judgment","inputs":["ticket.message"],"steps":["topic"]}` | `["record:/core/recorded/judgment/derivation","record:/core/recorded/judgment/lineage","record:/core/recorded/judgment/outcome"]` |
| matched | `"on_unresolved[0]"` | `null` | `null` | `["record:/core/recorded/judgment/trace/0"]` |
| rule_declaration | `{"action":{"escalate":{"reason":"ambiguous-topic"}},"reasons":"any","target":"step:topic"}` | `null` | `null` | `["plan:/definition/policy/on_unresolved/0"]` |
| adjustments | `["on_unresolved[0]"]` | `null` | `null` | `["record:/core/recorded/judgment/trace"]` |
| notices | `[{"kind":"uncalibrated_threshold","reason":"no calibration artifact exists for explicit_deadline; synthetic fixture","subject":"policy:adjustment:urgency"}]` | `null` | `null` | `["record:/core/recorded/judgment/notices"]` |
| judgment_derivation | `"mixed-derived"` | `{"bound":"judgment","class":"mixed-derived"}` | `{"bound":"judgment","inputs":["ticket.message"],"steps":["topic"]}` | `["record:/core/recorded/judgment/derivation","record:/core/recorded/judgment/lineage"]` |
| judgment_lineage | `{"inputs":["ticket.message"],"steps":["topic"]}` | `{"bound":"judgment","class":"mixed-derived"}` | `{"bound":"exact","inputs":["ticket.message"],"steps":["topic"]}` | `["record:/core/recorded/judgment/derivation","record:/core/recorded/judgment/lineage"]` |
| termination | `"judged"` | `null` | `null` | `["record:/termination"]` |
| cause | `{"unresolved":{"backend_unavailable":{"detail":"permanent: SYNTHETIC failure"}}}` | `null` | `null` | `["record:/core/recorded/steps/4/status"]` |
| request | `{"attempts":[{"artifact":"sha256:dd216677d2360b08b53c7717560a92058b61f699539c4936f0e70878a302ab3f","attempt_id":"explain-support/topic//1","backend_id":"synthetic-rules-support","cancellation":"not_requested","cost":{"bound":{"bounded":{"max_units":3}},"ch...` | `null` | `null` | `["record:/requests/0"]` |
| attempt | `{"artifact":"sha256:dd216677d2360b08b53c7717560a92058b61f699539c4936f0e70878a302ab3f","attempt_id":"explain-support/topic//1","backend_id":"synthetic-rules-support","cancellation":"not_requested","cost":{"bound":{"bounded":{"max_units":3}},"charge":{"obser...` | `null` | `null` | `["record:/requests/0/attempts/0"]` |
| transitions | `[{"stop":{"after":1,"reason":"permanent not retried further"}}]` | `null` | `null` | `["record:/requests/0/transitions"]` |
| addressed_to | not available: not recorded (facet addressed_to) | `null` | `null` | `[]` |
| remediation | not available: not recorded (facet remediation) | `null` | `null` | `[]` |
| caused_by | not available: not recorded (facet caused_by) | `null` | `null` | `[]` |
