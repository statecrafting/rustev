# Explanation: `"why_outcome"`

| Side | Form | Decision | Plan | Capture |
| --- | --- | --- | --- | --- |
| a | record | `"diff"` | `"sha256:011d1ba8a7f4a78f000393c6620b8bc218853b2fc05941cd2b6da6e44afaf47d"` | `null` |

The recorded outcome is a proposal.

| Claim | Value | Derivation | Lineage | Cites |
| --- | --- | --- | --- | --- |
| outcome | `{"propose":{"action":"route_ticket","params":{"failed_payments_30d":{"integer":2},"hours_since_first_failure":{"integer":5},"priority":{"enum":"high"},"queue":{"enum":"billing-priority"}}}}` | `{"bound":"judgment","class":"mixed-derived"}` | `{"bound":"judgment","inputs":["payments.events","ticket.message"],"steps":["explicit_deadline","failed_30d","failed_payments_30d","first_failure","frustration","hours_since_first_failure","topic"]}` | `["record:/core/recorded/judgment/derivation","record:/core/recorded/judgment/lineage","record:/core/recorded/judgment/outcome"]` |
| matched | `"rule:billing_priority"` | `null` | `null` | `["record:/core/recorded/judgment/trace/0"]` |
| rule_declaration | not available: plan not supplied | `null` | `null` | `[]` |
| adjustments | `["rule:billing_priority","adjustment:urgency"]` | `null` | `null` | `["record:/core/recorded/judgment/trace"]` |
| notices | `[{"kind":"uncalibrated_threshold","reason":"no calibration artifact exists for explicit_deadline; synthetic fixture","subject":"policy:adjustment:urgency"}]` | `null` | `null` | `["record:/core/recorded/judgment/notices"]` |
| judgment_derivation | `"mixed-derived"` | `{"bound":"judgment","class":"mixed-derived"}` | `{"bound":"judgment","inputs":["payments.events","ticket.message"],"steps":["explicit_deadline","failed_30d","failed_payments_30d","first_failure","frustration","hours_since_first_failure","topic"]}` | `["record:/core/recorded/judgment/derivation","record:/core/recorded/judgment/lineage"]` |
| judgment_lineage | `{"inputs":["payments.events","ticket.message"],"steps":["explicit_deadline","failed_30d","failed_payments_30d","first_failure","frustration","hours_since_first_failure","topic"]}` | `{"bound":"judgment","class":"mixed-derived"}` | `{"bound":"exact","inputs":["payments.events","ticket.message"],"steps":["explicit_deadline","failed_30d","failed_payments_30d","first_failure","frustration","hours_since_first_failure","topic"]}` | `["record:/core/recorded/judgment/derivation","record:/core/recorded/judgment/lineage"]` |
| termination | `"judged"` | `null` | `null` | `["record:/termination"]` |
| addressed_to | not available: not recorded (facet addressed_to) | `null` | `null` | `[]` |
| remediation | not available: not recorded (facet remediation) | `null` | `null` | `[]` |
| caused_by | not available: not recorded (facet caused_by) | `null` | `null` | `[]` |
