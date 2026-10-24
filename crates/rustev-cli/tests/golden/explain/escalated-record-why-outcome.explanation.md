# Explanation: `"why_outcome"`

| Side | Form | Decision | Plan | Capture |
| --- | --- | --- | --- | --- |
| a | record | `"explain-support"` | `"sha256:011d1ba8a7f4a78f000393c6620b8bc218853b2fc05941cd2b6da6e44afaf47d"` | `null` |

The recorded outcome is an escalation.

| Claim | Value | Derivation | Lineage | Cites |
| --- | --- | --- | --- | --- |
| outcome | `{"escalate":{"reason":"ambiguous-topic"}}` | `{"bound":"judgment","class":"mixed-derived"}` | `{"bound":"judgment","inputs":["ticket.message"],"steps":["topic"]}` | `["record:/core/recorded/judgment/derivation","record:/core/recorded/judgment/lineage","record:/core/recorded/judgment/outcome"]` |
| matched | `"on_unresolved[0]"` | `null` | `null` | `["record:/core/recorded/judgment/trace/0"]` |
| rule_declaration | not available: plan not supplied | `null` | `null` | `[]` |
| adjustments | `["on_unresolved[0]"]` | `null` | `null` | `["record:/core/recorded/judgment/trace"]` |
| notices | `[{"kind":"uncalibrated_threshold","reason":"no calibration artifact exists for explicit_deadline; synthetic fixture","subject":"policy:adjustment:urgency"}]` | `null` | `null` | `["record:/core/recorded/judgment/notices"]` |
| judgment_derivation | `"mixed-derived"` | `{"bound":"judgment","class":"mixed-derived"}` | `{"bound":"judgment","inputs":["ticket.message"],"steps":["topic"]}` | `["record:/core/recorded/judgment/derivation","record:/core/recorded/judgment/lineage"]` |
| judgment_lineage | `{"inputs":["ticket.message"],"steps":["topic"]}` | `{"bound":"judgment","class":"mixed-derived"}` | `{"bound":"exact","inputs":["ticket.message"],"steps":["topic"]}` | `["record:/core/recorded/judgment/derivation","record:/core/recorded/judgment/lineage"]` |
| termination | `"judged"` | `null` | `null` | `["record:/termination"]` |
| addressed_to | not available: not recorded (facet addressed_to) | `null` | `null` | `[]` |
| remediation | not available: not recorded (facet remediation) | `null` | `null` | `[]` |
| caused_by | not available: not recorded (facet caused_by) | `null` | `null` | `[]` |
