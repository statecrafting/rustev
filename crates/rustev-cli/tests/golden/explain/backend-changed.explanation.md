# Explanation: `"diff"`

| Side | Form | Decision | Plan | Capture |
| --- | --- | --- | --- | --- |
| a | bundle | `"diff"` | `"sha256:011d1ba8a7f4a78f000393c6620b8bc218853b2fc05941cd2b6da6e44afaf47d"` | `"complete"` |
| b | bundle | `"diff"` | `"sha256:011d1ba8a7f4a78f000393c6620b8bc218853b2fc05941cd2b6da6e44afaf47d"` | `"complete"` |

The compared decision values diverged.

| Claim | Value | Derivation | Lineage | Cites |
| --- | --- | --- | --- | --- |
| difference | `{"a":{"artifact":"sha256:dd216677d2360b08b53c7717560a92058b61f699539c4936f0e70878a302ab3f","instance":[],"output":{"logits":{"account_access":0.0,"billing":4.0,"integration_defect":0.0,"other":1.0}},"schema":"rustev.backend-output/1","step":"topic"},"a_sta...` | `{"bound":"exact","class":"mixed-derived"}` | `null` | `["a.bundle:/supplies/0/value/retention/retained/digest","a.supply/0:","b.bundle:/supplies/0/value/retention/retained/digest","b.supply/0:"]` |
