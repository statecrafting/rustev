# Explanation: `"diff"`

| Side | Form | Decision | Plan | Capture |
| --- | --- | --- | --- | --- |
| a | record | `"diff"` | `"sha256:011d1ba8a7f4a78f000393c6620b8bc218853b2fc05941cd2b6da6e44afaf47d"` | `null` |
| b | record | `"diff"` | `"sha256:011d1ba8a7f4a78f000393c6620b8bc218853b2fc05941cd2b6da6e44afaf47d"` | `null` |

The compared decision values diverged.

| Claim | Value | Derivation | Lineage | Cites |
| --- | --- | --- | --- | --- |
| difference | `{"a":{"resolved":{"as_of_ms":1790164740000,"provenance":"user-supplied","source":"synthetic"}},"a_state":"present","b":{"unresolved":{"missing_evidence":{"fields":["ticket.message"]}}},"b_state":"present","follows_from":[],"subject":"input:ticket.message",...` | `{"bound":"exact","class":"exact-derived"}` | `null` | `["a.record:/core/recorded/inputs/2/status","b.record:/core/recorded/inputs/2/status"]` |
| difference | `{"a":{"resolved":{"derivation":"model-derived","kind":"calibrated_probability"}},"a_state":"present","b":{"unresolved":{"missing_evidence":{"fields":["ticket.message"]}}},"b_state":"present","follows_from":["f1"],"subject":"step:topic#","unexplained":false...` | `{"bound":"exact","class":"mixed-derived"}` | `null` | `["a.record:/core/recorded/steps/4/status","b.record:/core/recorded/steps/4/status"]` |
