# Explanation: `"models"`

| Side | Form | Decision | Plan | Capture |
| --- | --- | --- | --- | --- |
| a | bundle | `"diff"` | `"sha256:011d1ba8a7f4a78f000393c6620b8bc218853b2fc05941cd2b6da6e44afaf47d"` | `"complete"` |

The record lists model requests, attempts and costs.

| Claim | Value | Derivation | Lineage | Cites |
| --- | --- | --- | --- | --- |
| request | `{"instance":[],"result":{"output":{"target":0}},"step":"topic"}` | `null` | `null` | `["record:/requests/0"]` |
| attempt | `{"artifact":"sha256:dd216677d2360b08b53c7717560a92058b61f699539c4936f0e70878a302ab3f","attempt_id":"diff/topic//1","backend_id":"synthetic-rules-support","cancellation":"not_requested","cost":{"bound":{"bounded":{"max_units":3}},"charge":{"observed":{"unit...` | `null` | `null` | `["plan:/steps/4/detail/semantic/descriptor","record:/requests/0/attempts/0"]` |
| transitions | `[]` | `null` | `null` | `["record:/requests/0/transitions"]` |
| request | `{"instance":[],"result":{"output":{"target":0}},"step":"frustration"}` | `null` | `null` | `["record:/requests/1"]` |
| attempt | `{"artifact":"sha256:dd216677d2360b08b53c7717560a92058b61f699539c4936f0e70878a302ab3f","attempt_id":"diff/frustration//1","backend_id":"synthetic-rules-support","cancellation":"not_requested","cost":{"bound":{"bounded":{"max_units":3}},"charge":{"observed":...` | `null` | `null` | `["plan:/steps/5/detail/semantic/descriptor","record:/requests/1/attempts/0"]` |
| transitions | `[]` | `null` | `null` | `["record:/requests/1/transitions"]` |
| request | `{"instance":[],"result":{"output":{"target":0}},"step":"explicit_deadline"}` | `null` | `null` | `["record:/requests/2"]` |
| attempt | `{"artifact":"sha256:dd216677d2360b08b53c7717560a92058b61f699539c4936f0e70878a302ab3f","attempt_id":"diff/explicit_deadline//1","backend_id":"synthetic-rules-support","cancellation":"not_requested","cost":{"bound":{"bounded":{"max_units":3}},"charge":{"obse...` | `null` | `null` | `["plan:/steps/6/detail/semantic/descriptor","record:/requests/2/attempts/0"]` |
| transitions | `[]` | `null` | `null` | `["record:/requests/2/transitions"]` |
| served_identity | not available: not recorded (facet served_identity) | `null` | `null` | `[]` |
| backend_cost | `{"estimated":0,"observed":9,"unknown":0}` | `null` | `null` | `["record:/requests/0/attempts/0/cost/charge","record:/requests/1/attempts/0/cost/charge","record:/requests/2/attempts/0/cost/charge"]` |
| cost_summary | `{"bound_violations":0,"estimated":0,"final_cost":"known","liability":0,"limit":0,"mode":"unlimited","observed":9,"within_guaranteed_cap":false}` | `null` | `null` | `["record:/cost"]` |
